//! `Jenkinsfile` for `jenkins` environments: a declarative pipeline the
//! execution service starts with `buildWithParameters` (`IMAGE`, `DIGEST`,
//! `VERSION`, `ENVIRONMENT`) and polls to its end (CONTRACT.md §22). Three
//! stages: verify what was asked for, deploy it, check the environment
//! answers. It deploys the same way the GitHub workflow would
//! ([`Target`](super::target::Target)); credentials are Jenkins "Secret
//! text" entries, one set per environment.

use super::{
    Output,
    input::{DeliveryInput, DeployerKind, EnvironmentSpec},
    target::{Plan, SettingDefault},
};

/// The repository's own `Jenkinsfile` is left alone: a second pipeline goes
/// next to it.
pub fn file_name(input: &DeliveryInput) -> &'static str {
    if input.base.exists("Jenkinsfile") {
        "Jenkinsfile.deploy"
    } else {
        "Jenkinsfile"
    }
}

/// A Groovy single-quoted string.
fn groovy(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// The Jenkins credential id of a secret variable: `KUBECONFIG_B64` →
/// `kubeconfig-b64`, suffixed with the environment's name at run time.
fn credential_id(name: &str) -> String {
    name.to_ascii_lowercase().replace('_', "-")
}

/// An `sh` step running `lines` under `set -eu`. Inside a Groovy `'''`
/// string only the backslash is special, so it is the one thing escaped.
fn sh_step(lines: &[&String], indent: &str) -> String {
    let body: String = lines
        .iter()
        .map(|line| format!("{indent}  {}\n", line.replace('\\', "\\\\")))
        .collect();
    format!("{indent}sh '''\n{indent}  set -eu\n{body}{indent}'''\n")
}

pub(super) fn generate(input: &DeliveryInput, out: &mut Output) {
    let environments = input.environments_of(DeployerKind::Jenkins);
    if environments.is_empty() {
        return;
    }
    let target = input.target.target;
    let plan = target.plan(&input.base, &input.workload);
    let file = file_name(input);
    let names = environments
        .iter()
        .map(|e| format!("`{}`", e.name))
        .collect::<Vec<_>>()
        .join(", ");
    out.file(
        input,
        file,
        "text",
        format!(
            "Jenkins pipeline that deploys one released image to one environment ({names}) on {}: verify, deploy, health check.",
            target.label()
        ),
        jenkinsfile(input, &environments, &plan),
    );

    out.note(format!(
        "`{file}` is started by SDI with the parameters `IMAGE`, `DIGEST`, `VERSION` and `ENVIRONMENT`; it deploys to {} because {}.",
        target.label(),
        input.target.evidence
    ));
    for env in &environments {
        let (url, job) = (env.setting("url"), env.setting("job"));
        out.note(format!(
            "`{}`: create a Pipeline job `{job}` on {url} whose script path is `{file}`, and run it once so Jenkins registers the parameters.{}",
            env.name,
            match (env.setting("user_secret"), env.setting("token_secret")) {
                ("", _) | (_, "") => " Set `user_secret` and `token_secret` on the environment to the workspace secrets holding a Jenkins user and its API token.".to_owned(),
                (user, token) => format!(
                    " SDI calls Jenkins with the workspace secrets `{user}` and `{token}` (a Jenkins user and its API token)."
                ),
            }
        ));
    }
    if !plan.credentials.is_empty() {
        for env in &environments {
            for credential in &plan.credentials {
                out.secret(&format!("{}-{}", credential_id(credential.name), env.name));
            }
        }
        out.note(format!(
            "Add Jenkins credentials of kind \"Secret text\" for each environment, with the environment's name as the suffix of the id: {}.",
            plan.credentials
                .iter()
                .map(|c| format!("`{}-<environment>` ({})", credential_id(c.name), c.what))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out.note(format!(
        "The Jenkins agent needs {} on its PATH.",
        target
            .tools()
            .iter()
            .map(|t| format!("`{t}`"))
            .collect::<Vec<_>>()
            .join(" and ")
    ));
}

fn jenkinsfile(input: &DeliveryInput, environments: &[&EnvironmentSpec], plan: &Plan) -> String {
    let target = input.target.target;
    let health: Vec<String> = environments
        .iter()
        .filter(|e| !e.health_url().is_empty())
        .map(|e| format!("  {}: {}", groovy(&e.name), groovy(&e.health_url())))
        .collect();
    let health_map = if health.is_empty() {
        "[:]".to_owned()
    } else {
        format!("[\n{}\n]", health.join(",\n"))
    };
    let choices = environments
        .iter()
        .map(|e| groovy(&e.name))
        .collect::<Vec<_>>()
        .join(", ");
    let settings: String = plan
        .settings
        .iter()
        .map(|s| match &s.default {
            SettingDefault::Text(text) => format!("    {} = {}\n", s.name, groovy(text)),
            SettingDefault::EnvironmentName => {
                format!("    {} = \"${{params.ENVIRONMENT}}\"\n", s.name)
            }
        })
        .collect();
    let image_tag = if target.needs_image_tag() {
        "    IMAGE_TAG = \"${params.DIGEST?.trim() && params.VERSION?.trim() ? params.VERSION.trim() + '@' + params.DIGEST.trim() : params.VERSION}\"\n"
    } else {
        ""
    };
    let commands: Vec<&String> = plan.login.iter().chain(&plan.deploy).collect();
    let deploy = if plan.credentials.is_empty() {
        sh_step(&commands, "        ")
    } else {
        let body = sh_step(&commands, "          ");
        let bindings = plan
            .credentials
            .iter()
            .map(|c| {
                format!(
                    "          string(credentialsId: \"{}-${{params.ENVIRONMENT}}\", variable: {})",
                    credential_id(c.name),
                    groovy(c.name)
                )
            })
            .collect::<Vec<_>>()
            .join(",\n");
        format!("        withCredentials([\n{bindings}\n        ]) {{\n{body}        }}\n")
    };
    format!(
        "// Deploys one released image of {app} to one environment.
// SDI starts this job (buildWithParameters) once a promotion has passed its
// policy gates and follows the build to its end. Nothing here builds an image.

def HEALTH_URLS = {health_map}

pipeline {{
  agent any

  options {{
    disableConcurrentBuilds()
    timeout(time: 30, unit: 'MINUTES')
    buildDiscarder(logRotator(numToKeepStr: '50'))
  }}

  parameters {{
    string(name: 'IMAGE', defaultValue: {image}, description: 'Image repository without a tag')
    string(name: 'DIGEST', defaultValue: '', description: 'Digest of the released image (sha256:...)')
    string(name: 'VERSION', defaultValue: '', description: 'Release version, which is also the image tag')
    choice(name: 'ENVIRONMENT', choices: [{choices}], description: 'Environment to deploy to')
  }}

  environment {{
    IMAGE = \"${{params.IMAGE}}\"
    IMAGE_REF = \"${{params.DIGEST?.trim() ? params.IMAGE.trim() + '@' + params.DIGEST.trim() : params.IMAGE.trim() + ':' + params.VERSION}}\"
{image_tag}{settings}  }}

  stages {{
    stage('Verify') {{
      steps {{
        script {{
          if (!params.IMAGE?.trim()) {{
            error('IMAGE is required')
          }}
          if (!params.DIGEST?.trim() && !params.VERSION?.trim()) {{
            error('A DIGEST or a VERSION is required')
          }}
          if (params.DIGEST?.trim() && !(params.DIGEST.trim() ==~ /sha256:[a-f0-9]{{64}}/)) {{
            error(\"DIGEST must be sha256 followed by 64 hex characters, got ${{params.DIGEST}}\")
          }}
        }}
        sh '''
          set -eu
          if command -v docker > /dev/null 2>&1; then
            docker manifest inspect \"$IMAGE_REF\" > /dev/null
          fi
        '''
      }}
    }}

    stage('Deploy') {{
      steps {{
{deploy}      }}
    }}

    stage('Health') {{
      when {{
        expression {{ HEALTH_URLS.containsKey(params.ENVIRONMENT) }}
      }}
      steps {{
        sh \"curl --retry 10 --retry-delay 6 --retry-all-errors -fsS '${{HEALTH_URLS[params.ENVIRONMENT]}}'\"
      }}
    }}
  }}

  post {{
    success {{
      echo \"Deployed ${{env.IMAGE_REF}} to ${{params.ENVIRONMENT}}\"
    }}
    failure {{
      echo \"Deploying ${{env.IMAGE_REF}} to ${{params.ENVIRONMENT}} failed\"
    }}
  }}
}}
",
        app = input.base.image,
        image = groovy(&input.release_image()),
    )
}
