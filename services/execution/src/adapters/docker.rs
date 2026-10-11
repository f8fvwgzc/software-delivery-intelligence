//! Real local execution on the host's Docker daemon through the `docker`
//! CLI (which honours `DOCKER_HOST`, so the compose `local-exec` profile only
//! has to mount the socket). Every run owns `SDI_RUN_DIR/<execution-id>/`:
//! `repo.checkout` fills it with the GitHub tarball at the scan commit,
//! `container.build` builds from it, `container.run` starts the image under
//! the name `sdi-<short id>` and polls its health path, `tests.run_local`
//! runs the configured command inside the image as `sdi-<short id>-cmd`,
//! `env.destroy_temporary` removes the containers and the directory. Every
//! path a step names (`dockerfile`, `context`, `test_dir`) is resolved
//! inside that directory and refused otherwise: the daemon reads the host's
//! filesystem, so a plan must not be able to point it at the home directory.
//! Every stdout/stderr line goes to the log sink; the last lines become the
//! raw output of a failure.

use std::{
    collections::{BTreeMap, VecDeque},
    io::Cursor,
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use reqwest::StatusCode;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::Command,
};
use uuid::Uuid;

use super::{
    Adapter, DOCKER, LogSink, RunContext, StepInput, StepOutcome, short_id, truncate_line,
};
use crate::domain::diagnosis;

const CHECKOUT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const BUILD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const RUN_TIMEOUT: Duration = Duration::from_secs(2 * 60);
const TESTS_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const DESTROY_TIMEOUT: Duration = Duration::from_secs(2 * 60);
/// How long `container.run` waits for the health path to answer.
const HEALTH_WAIT: Duration = Duration::from_secs(60);
const HEALTH_POLL: Duration = Duration::from_secs(1);
/// Lines of tool output kept for `Failure.raw_output`.
const TAIL_LINES: usize = 200;
const USER_AGENT: &str = concat!("sdi-execution/", env!("CARGO_PKG_VERSION"));

pub struct DockerAdapter {
    run_dir: PathBuf,
    github_token: Option<String>,
    github_api: String,
    http: reqwest::Client,
}

/// What a finished CLI invocation left behind.
struct Finished {
    success: bool,
    status: String,
    tail: Vec<String>,
}

enum CommandError {
    Timeout,
    Spawn(std::io::Error),
}

impl DockerAdapter {
    pub fn new(
        run_dir: impl Into<PathBuf>,
        github_token: Option<String>,
        github_api: &str,
    ) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .expect("reqwest client");
        Self {
            run_dir: run_dir.into(),
            github_token,
            github_api: github_api.trim_end_matches('/').to_owned(),
            http,
        }
    }

    pub fn workdir(&self, execution_id: Uuid) -> PathBuf {
        self.run_dir.join(execution_id.to_string())
    }

    fn container_name(execution_id: Uuid) -> String {
        format!("sdi-{}", short_id(execution_id))
    }

    /// The container `tests.run_local` and `deps.install` run their command
    /// in. Named so that a timeout, a cancel or a restart can remove it:
    /// killing the attached `docker run` client leaves the container running
    /// and `--rm` only fires when the command exits by itself.
    fn command_container_name(execution_id: Uuid) -> String {
        format!("sdi-{}-cmd", short_id(execution_id))
    }

    /// Per-step limit; `params.timeout_seconds` (set by the increase_timeout
    /// remediation) overrides the default.
    fn timeout_for(step: &StepInput, default: Duration) -> Duration {
        step.params
            .get("timeout_seconds")
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|s| *s > 0)
            .map_or(default, Duration::from_secs)
    }

    /// Runs a command, streaming every output line to the log sink and keeping
    /// the last `TAIL_LINES`. Kills the child on timeout (and on drop, so a
    /// cancelled step never leaves a build running).
    async fn run_command(
        logs: &LogSink,
        mut cmd: Command,
        timeout: Duration,
    ) -> Result<Finished, CommandError> {
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(CommandError::Spawn)?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let tail: Arc<Mutex<VecDeque<String>>> = Arc::default();

        let wait = async {
            tokio::join!(
                pump(stdout, logs, &tail),
                pump(stderr, logs, &tail),
                child.wait()
            )
            .2
        };
        match tokio::time::timeout(timeout, wait).await {
            Ok(Ok(status)) => Ok(Finished {
                success: status.success(),
                status: status.code().map_or_else(
                    || "killed by signal".to_owned(),
                    |c| format!("exit code {c}"),
                ),
                tail: tail.lock().expect("tail lock").iter().cloned().collect(),
            }),
            Ok(Err(error)) => Err(CommandError::Spawn(error)),
            Err(_) => {
                let _ = child.kill().await;
                Err(CommandError::Timeout)
            }
        }
    }

    /// Fire-and-forget CLI call whose failure does not matter (removing a
    /// container that is already gone).
    async fn best_effort(args: &[&str]) {
        let _ = Command::new("docker")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .status()
            .await;
    }

    fn image_for(step: &StepInput, ctx: &RunContext) -> Option<String> {
        step.params
            .get("image")
            .filter(|s| !s.trim().is_empty())
            .or_else(|| ctx.outputs.get("image"))
            .map(|s| s.trim().to_owned())
    }

    fn failed_run(code: &str, step: &StepInput, finished: &Finished) -> StepOutcome {
        let mut raw = finished.tail.join("\n");
        raw.push_str(&format!("\n{}: {}", step.action_type, finished.status));
        StepOutcome::failed(code, raw)
    }

    fn command_error(error: CommandError, what: &str) -> StepOutcome {
        match error {
            CommandError::Timeout => StepOutcome::failed(
                diagnosis::TIMEOUT,
                format!("{what} did not finish within its time limit; the process was killed"),
            ),
            CommandError::Spawn(error) => StepOutcome::failed(
                diagnosis::UNKNOWN,
                format!("could not start the docker CLI for {what}: {error}"),
            ),
        }
    }

    // --- steps ------------------------------------------------------------------

    async fn checkout(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        let Some(repo) = step
            .params
            .get("repo")
            .or_else(|| step.params.get("repository"))
            .map(|s| s.trim().trim_matches('/').to_owned())
            .filter(|s| s.split('/').count() == 2)
        else {
            return StepOutcome::failed(
                diagnosis::UNKNOWN,
                "params.repo (owner/name) is required for repo.checkout",
            );
        };
        let reference = step
            .params
            .get("sha")
            .or_else(|| step.params.get("commit"))
            .or_else(|| step.params.get("ref"))
            .map(|s| s.trim().to_owned())
            .unwrap_or_default();
        let workdir = self.workdir(ctx.execution_id);
        if let Err(error) = tokio::fs::create_dir_all(&workdir).await {
            return StepOutcome::failed(
                diagnosis::UNKNOWN,
                format!("could not create {}: {error}", workdir.display()),
            );
        }

        let url = if reference.is_empty() {
            format!("{}/repos/{repo}/tarball", self.github_api)
        } else {
            format!("{}/repos/{repo}/tarball/{reference}", self.github_api)
        };
        ctx.logs.line(format!("GET {url}"));
        let mut request = self
            .http
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .timeout(Self::timeout_for(step, CHECKOUT_TIMEOUT));
        if let Some(token) = &self.github_token {
            request = request.bearer_auth(token);
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) if error.is_timeout() => {
                return StepOutcome::failed(
                    diagnosis::TIMEOUT,
                    format!("downloading {url} timed out: {error}"),
                );
            }
            Err(error) => {
                return StepOutcome::failed(
                    diagnosis::UNKNOWN,
                    format!("downloading {url} failed: {error}"),
                );
            }
        };
        let status = response.status();
        ctx.logs.line(format!("GET {url} -> {status}"));
        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                return StepOutcome::Failed {
                    code: diagnosis::PERMISSION_DENIED.to_owned(),
                    raw_output: format!(
                        "GET {url} -> {status}: GITHUB_TOKEN {} read {repo}",
                        if self.github_token.is_some() {
                            "cannot"
                        } else {
                            "is not configured and anonymous access cannot"
                        }
                    ),
                    missing_permissions: vec!["repository:read".to_owned()],
                };
            }
            s if !s.is_success() => {
                return StepOutcome::failed(
                    diagnosis::UNKNOWN,
                    format!("GET {url} -> {status}: {}", body_excerpt(response).await),
                );
            }
            _ => {}
        }
        let bytes = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(error) => {
                return StepOutcome::failed(
                    diagnosis::UNKNOWN,
                    format!("reading the tarball of {repo} failed: {error}"),
                );
            }
        };
        ctx.logs
            .line(format!("downloaded {} bytes, unpacking", bytes.len()));

        let dest = workdir.clone();
        let unpacked = tokio::task::spawn_blocking(move || unpack_tarball(&bytes, &dest)).await;
        match unpacked {
            Ok(Ok(files)) => {
                ctx.logs
                    .line(format!("unpacked {files} files into {}", workdir.display()));
                // The proposal's generated files go on top of the checkout, so
                // container.build builds the Dockerfile the option proposes,
                // not only what the repository already has.
                match write_artifacts(&workdir, &ctx.artifacts).await {
                    Ok(0) => {}
                    Ok(written) => ctx.logs.line(format!(
                        "wrote {written} generated files on top of the checkout"
                    )),
                    Err(error) => {
                        return StepOutcome::failed(
                            diagnosis::UNKNOWN,
                            format!("could not write the generated files: {error}"),
                        );
                    }
                }
                let mut outputs =
                    BTreeMap::from([("workdir".to_owned(), workdir.display().to_string())]);
                if !reference.is_empty() {
                    outputs.insert("commit".to_owned(), reference.clone());
                }
                StepOutcome::Succeeded {
                    output: format!(
                        "checked out {repo}{} ({files} files) into {}",
                        if reference.is_empty() {
                            String::new()
                        } else {
                            format!("@{reference}")
                        },
                        workdir.display()
                    ),
                    outputs,
                }
            }
            Ok(Err(error)) => StepOutcome::failed(
                diagnosis::UNKNOWN,
                format!("unpacking the tarball of {repo} failed: {error}"),
            ),
            Err(error) => StepOutcome::failed(
                diagnosis::UNKNOWN,
                format!("unpacking the tarball of {repo} panicked: {error}"),
            ),
        }
    }

    async fn create_env(&self, ctx: &RunContext) -> StepOutcome {
        let workdir = self.workdir(ctx.execution_id);
        match tokio::fs::create_dir_all(&workdir).await {
            Ok(()) => StepOutcome::Succeeded {
                output: format!("temporary environment ready at {}", workdir.display()),
                outputs: BTreeMap::from([("workdir".to_owned(), workdir.display().to_string())]),
            },
            Err(error) => StepOutcome::failed(
                diagnosis::UNKNOWN,
                format!("could not create {}: {error}", workdir.display()),
            ),
        }
    }

    async fn build(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        let workdir = self.workdir(ctx.execution_id);
        if !workdir.is_dir() {
            return StepOutcome::failed(
                diagnosis::BUILD_FAILED,
                format!(
                    "{} does not exist: run repo.checkout before container.build",
                    workdir.display()
                ),
            );
        }
        let dockerfile = match inside_workdir(
            &workdir,
            step.params.get("dockerfile").map_or("./Dockerfile", |s| s),
            "params.dockerfile",
        ) {
            Ok(path) if path.is_file() => path,
            Ok(path) => {
                return StepOutcome::failed(
                    diagnosis::BUILD_FAILED,
                    format!("{} is not a file", path.display()),
                );
            }
            Err(error) => return StepOutcome::failed(diagnosis::BUILD_FAILED, error),
        };
        let context = match inside_workdir(
            &workdir,
            step.params.get("context").map_or(".", |s| s),
            "params.context",
        ) {
            Ok(path) if path.is_dir() => path,
            Ok(path) => {
                return StepOutcome::failed(
                    diagnosis::BUILD_FAILED,
                    format!("{} is not a directory", path.display()),
                );
            }
            Err(error) => return StepOutcome::failed(diagnosis::BUILD_FAILED, error),
        };
        let image = step
            .params
            .get("image")
            .filter(|s| !s.trim().is_empty())
            .map_or_else(
                || format!("sdi/{}", ctx.short_id()),
                |s| s.trim().to_owned(),
            );
        let tag = step
            .params
            .get("tag")
            .filter(|s| !s.trim().is_empty())
            .map_or("latest", |s| s.trim());
        let full = if image.contains(':') {
            image
        } else {
            format!("{image}:{tag}")
        };

        ctx.logs.line(format!(
            "docker build -t {full} -f {} {}  (in {})",
            dockerfile.display(),
            context.display(),
            workdir.display()
        ));
        let mut cmd = Command::new("docker");
        cmd.arg("build")
            .args(["-t", &full, "-f"])
            .arg(&dockerfile)
            .arg(&context)
            .current_dir(&workdir);
        match Self::run_command(&ctx.logs, cmd, Self::timeout_for(step, BUILD_TIMEOUT)).await {
            Ok(finished) if finished.success => StepOutcome::Succeeded {
                output: format!("{}\nbuilt {full}", finished.tail.join("\n")),
                outputs: BTreeMap::from([("image".to_owned(), full)]),
            },
            Ok(finished) => Self::failed_run(diagnosis::BUILD_FAILED, step, &finished),
            Err(error) => Self::command_error(error, "docker build"),
        }
    }

    async fn run_container(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        let Some(image) = Self::image_for(step, ctx) else {
            return StepOutcome::failed(
                diagnosis::UNKNOWN,
                "no image to run: build one with container.build first or set params.image",
            );
        };
        let port: u16 = match step
            .params
            .get("port")
            .map_or(Ok(8080), |p| p.trim().parse())
        {
            Ok(port) => port,
            Err(_) => {
                return StepOutcome::failed(diagnosis::UNKNOWN, "params.port is not a port number");
            }
        };
        let health_path = step
            .params
            .get("health_path")
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .map_or_else(
                || "/".to_owned(),
                |p| format!("/{}", p.trim_start_matches('/')),
            );
        let name = Self::container_name(ctx.execution_id);
        let timeout = Self::timeout_for(step, RUN_TIMEOUT);
        let started = tokio::time::Instant::now();

        // A retry must not trip over the previous attempt's container.
        Self::best_effort(&["rm", "-f", &name]).await;
        // No `--rm`: a container that dies on start must keep its logs so
        // the failure can say why; env.destroy_temporary removes it.
        let publish = format!("127.0.0.1:0:{port}");
        ctx.logs
            .line(format!("docker run -d -p {publish} --name {name} {image}"));
        let mut cmd = Command::new("docker");
        cmd.args(["run", "-d", "-p", &publish, "--name", &name, &image]);
        match Self::run_command(&ctx.logs, cmd, timeout).await {
            Ok(finished) if finished.success => {}
            Ok(finished) => return Self::failed_run(diagnosis::UNKNOWN, step, &finished),
            Err(error) => return Self::command_error(error, "docker run"),
        }

        let mapped = match self.mapped_port(&name, port).await {
            Some(mapped) => mapped,
            None => {
                let state = self.container_state(&name).await;
                let logs = self.container_logs(&name).await;
                Self::best_effort(&["rm", "-f", &name]).await;
                return StepOutcome::failed(
                    diagnosis::HEALTH_CHECK_FAILED,
                    format!(
                        "{name} exposes no host port for {port}/tcp (container {state})\n--- docker logs {name} ---\n{logs}"
                    ),
                );
            }
        };
        let base = format!("http://127.0.0.1:{mapped}");
        let health_url = format!("{base}{health_path}");
        ctx.logs
            .line(format!("{name} is up on {base}; polling {health_url}"));

        let deadline = started + timeout.min(HEALTH_WAIT + Duration::from_secs(5));
        let health_deadline = tokio::time::Instant::now() + HEALTH_WAIT;
        let mut last = String::from("no response yet");
        let mut polls = 0u32;
        while tokio::time::Instant::now() < health_deadline.min(deadline) {
            polls += 1;
            match self
                .http
                .get(&health_url)
                .timeout(Duration::from_secs(2))
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => {
                    ctx.logs.line(format!(
                        "GET {health_url} -> {} after {polls} polls",
                        response.status()
                    ));
                    return StepOutcome::Succeeded {
                        output: format!(
                            "{name} ({image}) is healthy: GET {health_url} -> {} ({:.1}s)",
                            response.status(),
                            started.elapsed().as_secs_f64()
                        ),
                        outputs: BTreeMap::from([
                            ("container_url".to_owned(), base),
                            ("image".to_owned(), image),
                            ("container".to_owned(), name),
                        ]),
                    };
                }
                Ok(response) => last = format!("GET {health_url} -> {}", response.status()),
                Err(error) => last = format!("GET {health_url} failed: {error}"),
            }
            // A process that already exited will never answer: report now
            // with its exit code instead of polling for a minute.
            let state = self.container_state(&name).await;
            if !state.starts_with("running") {
                last = format!("{last}; container {state}");
                break;
            }
            tokio::time::sleep(HEALTH_POLL).await;
        }
        ctx.logs
            .line(format!("{last} (giving up after {polls} polls)"));
        let logs = self.container_logs(&name).await;
        Self::best_effort(&["rm", "-f", &name]).await;
        StepOutcome::failed(
            diagnosis::HEALTH_CHECK_FAILED,
            format!("{last} after {polls} polls\n--- docker logs {name} ---\n{logs}"),
        )
    }

    /// `running`, or `exited with code N` for a container that stopped.
    async fn container_state(&self, name: &str) -> String {
        let output = Command::new("docker")
            .args([
                "inspect",
                "--format",
                "{{.State.Status}} {{.State.ExitCode}}",
                name,
            ])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await;
        let text = match output {
            Ok(output) if output.status.success() => {
                String::from_utf8_lossy(&output.stdout).trim().to_owned()
            }
            _ => return "state unknown".to_owned(),
        };
        match text.split_once(' ') {
            Some(("running", _)) => "running".to_owned(),
            Some((status, code)) => format!("{status} with code {code}"),
            None => text,
        }
    }

    async fn mapped_port(&self, name: &str, port: u16) -> Option<u16> {
        let output = Command::new("docker")
            .args(["port", name, &format!("{port}/tcp")])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .ok()?;
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.trim().rsplit(':').next()?.parse::<u16>().ok())
            .next()
    }

    async fn container_logs(&self, name: &str) -> String {
        let output = Command::new("docker")
            .args(["logs", "--tail", &TAIL_LINES.to_string(), name])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await;
        match output {
            Ok(output) => {
                let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
                text.push_str(&String::from_utf8_lossy(&output.stderr));
                text.lines()
                    .map(truncate_line)
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            Err(error) => format!("(docker logs failed: {error})"),
        }
    }

    async fn run_in_image(
        &self,
        step: &StepInput,
        ctx: &RunContext,
        command: &str,
        timeout: Duration,
        failure_code: &str,
    ) -> StepOutcome {
        let Some(image) = Self::image_for(step, ctx) else {
            return StepOutcome::failed(
                diagnosis::UNKNOWN,
                "no image to run in: build one with container.build first or set params.image",
            );
        };
        let name = Self::command_container_name(ctx.execution_id);
        Self::best_effort(&["rm", "-f", &name]).await;
        ctx.logs.line(format!(
            "docker run --rm --name {name} {image} sh -c {command:?}"
        ));
        let mut cmd = Command::new("docker");
        cmd.args(["run", "--rm", "--name", &name, &image, "sh", "-c", command]);
        Self::finish_command(step, ctx, &name, cmd, timeout, failure_code, || {
            format!("{command:?} in {image}")
        })
        .await
    }

    /// Runs a command container to its end, removing it when the step's time
    /// limit kills the client so no test keeps spinning on the daemon.
    async fn finish_command(
        step: &StepInput,
        ctx: &RunContext,
        name: &str,
        cmd: Command,
        timeout: Duration,
        failure_code: &str,
        what: impl FnOnce() -> String,
    ) -> StepOutcome {
        match Self::run_command(&ctx.logs, cmd, timeout).await {
            Ok(finished) if finished.success => StepOutcome::ok(format!(
                "{}\n{}: {}",
                finished.tail.join("\n"),
                what(),
                finished.status
            )),
            Ok(finished) => Self::failed_run(failure_code, step, &finished),
            Err(error) => {
                if matches!(error, CommandError::Timeout) {
                    ctx.logs.line(format!("docker rm -f {name}"));
                    Self::best_effort(&["rm", "-f", name]).await;
                }
                Self::command_error(error, &what())
            }
        }
    }

    async fn run_tests(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        if step.params.get("skip_tests").is_some_and(|v| v == "true") {
            return StepOutcome::ok("tests skipped by remediation");
        }
        let Some(command) = step
            .params
            .get("test_command")
            .map(|c| c.trim())
            .filter(|c| !c.is_empty())
        else {
            return StepOutcome::failed(
                diagnosis::TESTS_FAILED,
                "no test command configured: set test.command in .sdi.yml or params.test_command",
            );
        };
        // A compiled stack's runtime image carries the binary, not the
        // toolchain, so `cargo test` cannot run inside it: the plan then names
        // a toolchain image (params.test_image) and the tests run against the
        // checked-out source mounted into it.
        if let Some(test_image) = step
            .params
            .get("test_image")
            .map(|i| i.trim())
            .filter(|i| !i.is_empty())
        {
            return self.run_in_toolchain(step, ctx, test_image, command).await;
        }
        self.run_in_image(
            step,
            ctx,
            command,
            Self::timeout_for(step, TESTS_TIMEOUT),
            diagnosis::TESTS_FAILED,
        )
        .await
    }

    /// `docker run --rm --name <cmd> -v <workdir>/<test_dir>:/src -w /src <image> sh -c <command>`.
    async fn run_in_toolchain(
        &self,
        step: &StepInput,
        ctx: &RunContext,
        image: &str,
        command: &str,
    ) -> StepOutcome {
        let workdir = self.workdir(ctx.execution_id);
        if !workdir.is_dir() {
            return StepOutcome::failed(
                diagnosis::TESTS_FAILED,
                format!(
                    "{} does not exist: run repo.checkout before tests.run_local",
                    workdir.display()
                ),
            );
        }
        let test_dir = step
            .params
            .get("test_dir")
            .map(|d| d.trim().trim_matches('/'))
            .filter(|d| !d.is_empty())
            .unwrap_or(".");
        let source = match inside_workdir(&workdir, test_dir, "params.test_dir") {
            Ok(path) if path.is_dir() => path,
            Ok(path) => {
                return StepOutcome::failed(
                    diagnosis::TESTS_FAILED,
                    format!("{} is not a directory", path.display()),
                );
            }
            Err(error) => return StepOutcome::failed(diagnosis::TESTS_FAILED, error),
        };
        let name = Self::command_container_name(ctx.execution_id);
        Self::best_effort(&["rm", "-f", &name]).await;
        let mount = format!("{}:/src", source.display());
        ctx.logs.line(format!(
            "docker run --rm --name {name} -v {mount} -w /src {image} sh -c {command:?}"
        ));
        let mut cmd = Command::new("docker");
        cmd.args([
            "run", "--rm", "--name", &name, "-v", &mount, "-w", "/src", image, "sh", "-c", command,
        ]);
        Self::finish_command(
            step,
            ctx,
            &name,
            cmd,
            Self::timeout_for(step, TESTS_TIMEOUT),
            diagnosis::TESTS_FAILED,
            || format!("{command:?} in {image} against {test_dir}"),
        )
        .await
    }

    async fn install_deps(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        match step
            .params
            .get("install_command")
            .map(|c| c.trim())
            .filter(|c| !c.is_empty())
        {
            Some(command) => {
                self.run_in_image(
                    step,
                    ctx,
                    command,
                    Self::timeout_for(step, TESTS_TIMEOUT),
                    diagnosis::DEPENDENCY_CONFLICT,
                )
                .await
            }
            None => StepOutcome::ok(
                "dependencies are installed by the image build; no install_command configured",
            ),
        }
    }

    async fn destroy(&self, ctx: &RunContext) -> StepOutcome {
        let name = Self::container_name(ctx.execution_id);
        let command_name = Self::command_container_name(ctx.execution_id);
        let workdir = self.workdir(ctx.execution_id);
        ctx.logs.line(format!("docker rm -f {name} {command_name}"));
        let removed = tokio::time::timeout(
            DESTROY_TIMEOUT,
            Self::best_effort(&["rm", "-f", &name, &command_name]),
        )
        .await
        .is_ok();
        ctx.logs.line(format!("rm -rf {}", workdir.display()));
        let dir = match tokio::fs::remove_dir_all(&workdir).await {
            Ok(()) => "removed",
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => "already gone",
            Err(error) => {
                return StepOutcome::failed(
                    diagnosis::UNKNOWN,
                    format!("could not remove {}: {error}", workdir.display()),
                );
            }
        };
        if !removed {
            return StepOutcome::failed(
                diagnosis::TIMEOUT,
                format!("docker rm -f {name} did not finish within its time limit"),
            );
        }
        StepOutcome::ok(format!(
            "container {name} removed; workdir {} {dir}",
            workdir.display()
        ))
    }
}

/// Reads one pipe line by line into the sink and the tail ring.
async fn pump<R: AsyncRead + Unpin>(
    reader: Option<R>,
    logs: &LogSink,
    tail: &Arc<Mutex<VecDeque<String>>>,
) {
    let Some(reader) = reader else {
        return;
    };
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let line = truncate_line(&line);
        logs.line(line.clone());
        let mut tail = tail.lock().expect("tail lock");
        if tail.len() == TAIL_LINES {
            tail.pop_front();
        }
        tail.push_back(line);
    }
}

async fn body_excerpt(response: reqwest::Response) -> String {
    let text = response.text().await.unwrap_or_default();
    truncate_line(text.trim())
}

/// Resolves a step parameter naming a file or directory inside the workdir.
/// Absolute paths and `..` are refused up front; both sides are then
/// canonicalised so a symlink in the checkout cannot point the daemon at a
/// host directory either (CONTRACT.md §5.4: builds happen in
/// `.run/<execution>/`). Returns the canonical absolute path.
fn inside_workdir(workdir: &Path, raw: &str, what: &str) -> Result<PathBuf, String> {
    let relative = Path::new(raw.trim());
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(format!(
            "{what} {raw:?} must be a relative path inside the checkout"
        ));
    }
    let root = workdir
        .canonicalize()
        .map_err(|error| format!("{}: {error}", workdir.display()))?;
    let target = root
        .join(relative)
        .canonicalize()
        .map_err(|error| format!("{what} {raw:?}: {error}"))?;
    if !target.starts_with(&root) {
        return Err(format!(
            "{what} {raw:?} resolves outside the checkout ({})",
            target.display()
        ));
    }
    Ok(target)
}

/// Refuses a path that goes through a symlink anywhere under the workdir.
/// The checkout is untrusted: a repository that commits
/// `Dockerfile -> /etc/hosts` must not get the generated Dockerfile written
/// onto the host when the artifact lands on top of it.
async fn refuse_symlinks(workdir: &Path, relative: &Path) -> std::io::Result<()> {
    let mut current = workdir.to_path_buf();
    for component in relative.components() {
        current.push(component);
        match tokio::fs::symlink_metadata(&current).await {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "{} is a symbolic link in the checkout; refusing to write through it",
                        current.display()
                    ),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Writes the execution's artifacts under `workdir`. A path that leaves the
/// workdir (`..`, absolute) or crosses a symlink is refused, like a tarball
/// entry would be.
async fn write_artifacts(
    workdir: &Path,
    artifacts: &BTreeMap<String, String>,
) -> std::io::Result<usize> {
    let mut written = 0usize;
    for (path, content) in artifacts {
        let relative = Path::new(path.trim_start_matches("./"));
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("artifact path {path:?} is not relative to the repository"),
            ));
        }
        refuse_symlinks(workdir, relative).await?;
        let target = workdir.join(relative);
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&target, content).await?;
        written += 1;
    }
    Ok(written)
}

/// Unpacks a GitHub tarball into `dest`, dropping the `<owner>-<repo>-<sha>/`
/// top-level directory. Entries that would escape `dest` are skipped, and so
/// are symlinks and anything else that is not a plain file or directory: a
/// build needs none of them and a link target is a path on the host.
fn unpack_tarball(bytes: &[u8], dest: &Path) -> std::io::Result<usize> {
    let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let mut files = 0usize;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let relative: PathBuf = entry.path()?.components().skip(1).collect();
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            continue;
        }
        let target = dest.join(&relative);
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if !kind.is_file() {
            continue;
        }
        let mut file = std::fs::File::create(&target)?;
        std::io::copy(&mut entry, &mut file)?;
        #[cfg(unix)]
        if let Ok(mode) = entry.header().mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode));
        }
        files += 1;
    }
    Ok(files)
}

#[async_trait]
impl Adapter for DockerAdapter {
    fn name(&self) -> &'static str {
        DOCKER
    }

    async fn run(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome {
        match step.action_type.as_str() {
            "repo.checkout" => self.checkout(step, ctx).await,
            "env.create_temporary" => self.create_env(ctx).await,
            "container.build" => self.build(step, ctx).await,
            "container.run" => self.run_container(step, ctx).await,
            "tests.run_local" => self.run_tests(step, ctx).await,
            "deps.install" => self.install_deps(step, ctx).await,
            "env.destroy_temporary" => self.destroy(ctx).await,
            other => StepOutcome::failed(
                diagnosis::UNKNOWN,
                format!("the docker adapter cannot run {other}"),
            ),
        }
    }

    async fn stop(&self, execution_id: Uuid) {
        let name = Self::container_name(execution_id);
        let command_name = Self::command_container_name(execution_id);
        let _ = tokio::time::timeout(
            DESTROY_TIMEOUT,
            Self::best_effort(&["rm", "-f", &name, &command_name]),
        )
        .await;
    }

    async fn cleanup(&self, execution_id: Uuid) {
        self.stop(execution_id).await;
        let workdir = self.workdir(execution_id);
        if let Err(error) = tokio::fs::remove_dir_all(&workdir).await
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(execution = %execution_id, path = %workdir.display(), %error, "could not remove workdir");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn step(action_type: &str, params: &[(&str, &str)]) -> StepInput {
        StepInput {
            key: action_type.replace('.', "-"),
            title: action_type.to_owned(),
            action_type: action_type.to_owned(),
            adapter: DOCKER.to_owned(),
            params: params
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            attempt: 1,
        }
    }

    enum Entry<'a> {
        File(&'a str, &'a str),
        Link(&'a str, &'a str),
    }

    fn tarball(entries: &[Entry<'_>]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for entry in entries {
            let mut header = tar::Header::new_gnu();
            let (path, content) = match entry {
                Entry::File(path, content) => {
                    header.set_size(content.len() as u64);
                    header.set_mode(0o644);
                    (*path, *content)
                }
                Entry::Link(path, target) => {
                    header.set_entry_type(tar::EntryType::Symlink);
                    header.set_size(0);
                    header.set_mode(0o777);
                    header.set_link_name(target).unwrap();
                    (*path, "")
                }
            };
            // The builder refuses `..` itself, which is exactly what a hostile
            // archive would not do: write the raw name to get one past it.
            let name = &mut header.as_gnu_mut().unwrap().name;
            name[..path.len()].copy_from_slice(path.as_bytes());
            header.set_cksum();
            builder.append(&header, content.as_bytes()).unwrap();
        }
        let tar = builder.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar).unwrap();
        gz.finish().unwrap()
    }

    #[test]
    fn tarballs_lose_their_top_level_dir_and_cannot_escape() {
        let dir = std::env::temp_dir().join(format!("sdi-unpack-{}", Uuid::now_v7()));
        let bytes = tarball(&[
            Entry::File("owner-repo-abc123/Dockerfile", "FROM scratch\n"),
            Entry::File("owner-repo-abc123/src/main.rs", "fn main() {}\n"),
            Entry::File("owner-repo-abc123/../escape.txt", "nope"),
            // A committed symlink to a host path must not be recreated.
            Entry::Link("owner-repo-abc123/.github", "/etc"),
            Entry::Link("owner-repo-abc123/README.md", "../../outside.md"),
        ]);
        let files = unpack_tarball(&bytes, &dir).unwrap();
        assert_eq!(files, 2);
        assert_eq!(
            std::fs::read_to_string(dir.join("Dockerfile")).unwrap(),
            "FROM scratch\n"
        );
        assert!(dir.join("src/main.rs").is_file());
        assert!(!dir.parent().unwrap().join("escape.txt").exists());
        assert!(
            std::fs::symlink_metadata(dir.join(".github")).is_err(),
            ".github symlink was recreated"
        );
        assert!(std::fs::symlink_metadata(dir.join("README.md")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn artifacts_are_not_written_through_symlinks_in_the_checkout() {
        let base = std::env::temp_dir().join(format!("sdi-artifacts-{}", Uuid::now_v7()));
        let outside = base.join("outside");
        let workdir = base.join("work");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(workdir.join("ok")).unwrap();
        std::fs::write(outside.join("rc"), "keep me\n").unwrap();
        std::os::unix::fs::symlink(outside.join("rc"), workdir.join("Dockerfile")).unwrap();
        std::os::unix::fs::symlink(&outside, workdir.join(".github")).unwrap();

        let file_link = BTreeMap::from([("Dockerfile".to_owned(), "FROM x\n".to_owned())]);
        let error = write_artifacts(&workdir, &file_link).await.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput, "{error}");
        assert_eq!(
            std::fs::read_to_string(outside.join("rc")).unwrap(),
            "keep me\n"
        );

        let dir_link = BTreeMap::from([(".github/workflows/ci.yml".to_owned(), "x".to_owned())]);
        assert!(write_artifacts(&workdir, &dir_link).await.is_err());
        assert!(!outside.join("workflows").exists());

        // Plain paths, new or existing, still work.
        let fine = BTreeMap::from([
            ("ok/compose.yml".to_owned(), "services: {}\n".to_owned()),
            ("new/dir/file.txt".to_owned(), "hi".to_owned()),
        ]);
        assert_eq!(write_artifacts(&workdir, &fine).await.unwrap(), 2);
        assert!(workdir.join("new/dir/file.txt").is_file());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn build_and_test_paths_cannot_leave_the_workdir() {
        let base = std::env::temp_dir().join(format!("sdi-paths-{}", Uuid::now_v7()));
        let outside = base.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("Dockerfile"), "FROM scratch\n").unwrap();
        let adapter = DockerAdapter::new(base.join("run"), None, super::super::github::DEFAULT_API);
        let ctx = RunContext {
            execution_id: Uuid::now_v7(),
            ..Default::default()
        };
        let workdir = adapter.workdir(ctx.execution_id);
        std::fs::create_dir_all(workdir.join("app")).unwrap();
        std::fs::write(workdir.join("Dockerfile"), "FROM scratch\n").unwrap();
        std::fs::write(workdir.join("app/Dockerfile"), "FROM scratch\n").unwrap();
        std::os::unix::fs::symlink(&outside, workdir.join("linked")).unwrap();

        let refused = |outcome: StepOutcome, code: &str| match outcome {
            StepOutcome::Failed {
                code: got,
                raw_output,
                ..
            } => {
                assert_eq!(got, code, "{raw_output}");
                raw_output
            }
            other => panic!("{other:?}"),
        };
        // None of these reach the docker CLI: the refusal names the parameter.
        for (key, value) in [
            ("dockerfile", "../outside/Dockerfile"),
            ("dockerfile", outside.join("Dockerfile").to_str().unwrap()),
            ("dockerfile", "linked/Dockerfile"),
            ("context", "/"),
            ("context", "../outside"),
            ("context", "linked"),
        ] {
            let raw = refused(
                adapter
                    .run(&step("container.build", &[(key, value)]), &ctx)
                    .await,
                diagnosis::BUILD_FAILED,
            );
            assert!(
                raw.contains(&format!("params.{key}")),
                "{key}={value}: {raw}"
            );
        }
        let raw = refused(
            adapter
                .run(
                    &step(
                        "tests.run_local",
                        &[
                            ("test_command", "true"),
                            ("test_image", "rust:1-slim"),
                            ("test_dir", "../outside"),
                        ],
                    ),
                    &ctx,
                )
                .await,
            diagnosis::TESTS_FAILED,
        );
        assert!(raw.contains("params.test_dir"), "{raw}");
        assert!(
            inside_workdir(&workdir, "./app/Dockerfile", "x")
                .unwrap()
                .ends_with("app/Dockerfile")
        );
        assert!(inside_workdir(&workdir, "linked/Dockerfile", "x").is_err());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn step_timeouts_follow_the_params() {
        let default = Duration::from_secs(10);
        assert_eq!(
            DockerAdapter::timeout_for(&step("container.build", &[]), default),
            default
        );
        assert_eq!(
            DockerAdapter::timeout_for(
                &step("container.build", &[("timeout_seconds", "600")]),
                default
            ),
            Duration::from_secs(600)
        );
        assert_eq!(
            DockerAdapter::timeout_for(
                &step("container.build", &[("timeout_seconds", "x")]),
                default
            ),
            default
        );
    }

    #[tokio::test]
    async fn steps_without_prerequisites_fail_with_a_clear_code() {
        let dir = std::env::temp_dir().join(format!("sdi-docker-{}", Uuid::now_v7()));
        let adapter = DockerAdapter::new(&dir, None, super::super::github::DEFAULT_API);
        let ctx = RunContext {
            execution_id: Uuid::now_v7(),
            ..Default::default()
        };
        match adapter.run(&step("container.build", &[]), &ctx).await {
            StepOutcome::Failed {
                code, raw_output, ..
            } => {
                assert_eq!(code, diagnosis::BUILD_FAILED);
                assert!(raw_output.contains("repo.checkout"));
            }
            other => panic!("{other:?}"),
        }
        match adapter.run(&step("container.run", &[]), &ctx).await {
            StepOutcome::Failed { code, .. } => assert_eq!(code, diagnosis::UNKNOWN),
            other => panic!("{other:?}"),
        }
        match adapter
            .run(&step("tests.run_local", &[("image", "x")]), &ctx)
            .await
        {
            StepOutcome::Failed {
                code, raw_output, ..
            } => {
                assert_eq!(code, diagnosis::TESTS_FAILED);
                assert!(raw_output.contains("test.command"));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            adapter
                .run(&step("tests.run_local", &[("skip_tests", "true")]), &ctx)
                .await,
            StepOutcome::Succeeded { .. }
        ));
        match adapter.run(&step("repo.checkout", &[]), &ctx).await {
            StepOutcome::Failed { raw_output, .. } => assert!(raw_output.contains("params.repo")),
            other => panic!("{other:?}"),
        }
        match adapter.run(&step("deploy.production", &[]), &ctx).await {
            StepOutcome::Failed { code, .. } => assert_eq!(code, diagnosis::UNKNOWN),
            other => panic!("{other:?}"),
        }
    }

    /// Read-only against GitHub: `SDI_TEST_GITHUB=1 GITHUB_TOKEN=... cargo test -p sdi-execution`.
    #[tokio::test]
    async fn checks_out_a_real_repository_tarball() {
        if std::env::var("SDI_TEST_GITHUB").ok().as_deref() != Some("1") {
            eprintln!("skipping: SDI_TEST_GITHUB is not 1");
            return;
        }
        let token = std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.is_empty());
        let dir = std::env::temp_dir().join(format!("sdi-checkout-{}", Uuid::now_v7()));
        let adapter = DockerAdapter::new(&dir, token, super::super::github::DEFAULT_API);
        let ctx = RunContext {
            execution_id: Uuid::now_v7(),
            ..Default::default()
        };
        let repo = std::env::var("SDI_TEST_REPO").unwrap_or_else(|_| "f8fvwgzc/view-engine".into());
        let outcome = adapter
            .run(&step("repo.checkout", &[("repo", &repo)]), &ctx)
            .await;
        let StepOutcome::Succeeded { outputs, output } = outcome else {
            panic!("{outcome:?}");
        };
        let workdir = PathBuf::from(&outputs["workdir"]);
        assert!(
            workdir.join("Dockerfile").is_file() || workdir.join("README.md").is_file(),
            "{output}"
        );
        assert!(!workdir.join(".sdi-source.tar.gz").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Needs a Docker daemon: `SDI_TEST_DOCKER=1 cargo test -p sdi-execution`.
    #[tokio::test]
    async fn builds_runs_checks_health_tests_and_destroys_on_the_real_daemon() {
        if std::env::var("SDI_TEST_DOCKER").ok().as_deref() != Some("1") {
            eprintln!("skipping: SDI_TEST_DOCKER is not 1");
            return;
        }
        let dir = std::env::temp_dir().join(format!("sdi-docker-{}", Uuid::now_v7()));
        let adapter = DockerAdapter::new(&dir, None, super::super::github::DEFAULT_API);
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4096);
        let mut ctx = RunContext {
            execution_id: Uuid::now_v7(),
            logs: LogSink::new(log_tx),
            ..Default::default()
        };

        // A workdir with an inline Dockerfile stands in for repo.checkout.
        let workdir = adapter.workdir(ctx.execution_id);
        std::fs::create_dir_all(&workdir).unwrap();
        std::fs::write(
            workdir.join("Dockerfile"),
            "FROM nginxinc/nginx-unprivileged:1.28-alpine\n",
        )
        .unwrap();

        let built = adapter.run(&step("container.build", &[]), &ctx).await;
        let StepOutcome::Succeeded { outputs, .. } = built else {
            panic!("build: {built:?}");
        };
        assert_eq!(outputs["image"], format!("sdi/{}:latest", ctx.short_id()));
        ctx.outputs.extend(outputs);

        let ran = adapter
            .run(
                &step("container.run", &[("port", "8080"), ("health_path", "/")]),
                &ctx,
            )
            .await;
        let StepOutcome::Succeeded { outputs, .. } = ran else {
            panic!("run: {ran:?}");
        };
        assert!(outputs["container_url"].starts_with("http://127.0.0.1:"));
        assert_eq!(outputs["container"], format!("sdi-{}", ctx.short_id()));
        ctx.outputs.extend(outputs);

        let tested = adapter
            .run(
                &step(
                    "tests.run_local",
                    &[("test_command", "nginx -v && test -d /etc/nginx")],
                ),
                &ctx,
            )
            .await;
        assert!(
            matches!(tested, StepOutcome::Succeeded { .. }),
            "{tested:?}"
        );
        let failing = adapter
            .run(
                &step(
                    "tests.run_local",
                    &[("test_command", "echo boom >&2; exit 3")],
                ),
                &ctx,
            )
            .await;
        match failing {
            StepOutcome::Failed {
                code, raw_output, ..
            } => {
                assert_eq!(code, diagnosis::TESTS_FAILED);
                assert!(raw_output.contains("boom"), "{raw_output}");
                assert!(raw_output.contains("exit code 3"), "{raw_output}");
            }
            other => panic!("{other:?}"),
        }

        let destroyed = adapter.run(&step("env.destroy_temporary", &[]), &ctx).await;
        assert!(
            matches!(destroyed, StepOutcome::Succeeded { .. }),
            "{destroyed:?}"
        );
        assert!(!workdir.exists());

        let mut lines = Vec::new();
        while let Ok(line) = log_rx.try_recv() {
            lines.push(line);
        }
        assert!(
            lines.iter().any(|l| l.starts_with("docker build")),
            "{lines:?}"
        );
        assert!(lines.iter().any(|l| l.contains("boom")), "{lines:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
