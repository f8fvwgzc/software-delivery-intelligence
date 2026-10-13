# Getting started on a new machine

From a fresh clone to "SDI reviews my pull requests, writes the fixes it
can, and merges when I approve". About fifteen minutes, most of it the
first build.

This is the path for one person with the GitHub CLI. An organisation that
wants sign-in for everyone and webhooks installs the GitHub App instead:
[`github-app.md`](github-app.md). Running the stack in Docker for others to
reach is [`self-hosting.md`](self-hosting.md).

## 1. What you need

macOS, Linux, or Windows through WSL2, with:

| Tool | Why |
|---|---|
| Docker with Compose | Postgres runs in a container |
| Rust 1.88 or newer | the five services |
| `protoc` | the gRPC contracts are compiled at build time |
| Node 22 and pnpm | the web app |
| GitHub CLI (`gh`) | SDI uses your `gh` login to read and write your repositories |

About 15 GB of free disk: the first build writes roughly 12 GB into
`target/`.

## 2. Clone and check

```sh
git clone git@github.com:f8fvwgzc/software-delivery-intelligence.git
cd software-delivery-intelligence
make doctor
```

`make doctor` checks every tool, your GitHub login and its scopes, the
model settings, disk and ports. For anything missing it prints the command
that fixes it. It changes nothing and never prints a token.

## 3. Sign in to GitHub

```sh
gh auth login
gh auth refresh -s workflow
```

The default login gives the `repo` scope, which covers reading private
repositories, opening and merging pull requests, and reading Dependabot
alerts. The second command adds `workflow`, which GitHub requires for any
pull request that adds or changes a file under `.github/workflows/`.

For GitHub Enterprise Server, log in to that host
(`gh auth login --hostname ghe.example.com`) and put
`GITHUB_API_URL=https://ghe.example.com/api/v3` in `.env`.

SDI can do exactly what that login can do, and nothing more. To limit it
to a few repositories, use a fine-grained token in `.env` as
`GITHUB_TOKEN` instead (Contents, Pull requests: read and write; Metadata,
Actions, Deployments, Checks, Commit statuses, Dependabot alerts: read).

## 4. Add a model key (optional)

Without a key SDI still scans, proposes, reviews and merges: the review
rules and the ranking are deterministic. A model adds prose (review
summaries, negotiation replies, failure diagnoses) and, if you allow it,
drafted code fixes.

```sh
cp .env.example .env
```

Then, for OpenAI, set these three lines in `.env`:

```sh
LLM_PROVIDER=openai
LLM_API_KEY=sk-...
LLM_MODEL=<a model your key can use>
```

`LLM_MODEL` is required for every provider except Anthropic. `.env` is
git-ignored. Anthropic, Azure OpenAI, Ollama and any other endpoint that
speaks the OpenAI chat API work the same way; `.env.example` has a block
for each.

## 5. Start

```sh
make dev
```

It starts Postgres, builds and runs the services, installs the web
dependencies on the first run and serves the app at
<http://localhost:5173>. Opening it signs you in as your `gh` account with
a personal workspace. Stop everything with Ctrl-C.

## 6. Switch on what you want

Everything below is off until you turn it on. Each switch is independent.

| What you want | Where | Notes |
|---|---|---|
| The model writes summaries, replies and diagnoses | **Workspace, Data & privacy**: turn the reasoner on | Sends structured context only, never file contents. Then press **Test connection** on **Settings**: it makes one tiny call and says "Connected", or names exactly what is missing (the switch, the key, the model name, or what the provider answered). |
| The model drafts code fixes | **Workspace, Data & privacy**: Source lines to the model, "Lines a finding points at" | A second switch. Sends only the lines a finding points at, at most 120, with secret-looking lines replaced. |
| Open pull requests are reviewed on their own | **Settings, Automation**: Review pull requests automatically | A watcher looks at your connected repositories every minute. No webhook needed. Reviews wait for you; nothing is published. The same panel shows when it last looked. |
| Fixes are drafted during the review | **Settings, Automation**: Draft fixes with the model | Up to ten per review. Needs the two model switches above. |
| Dependabot pull requests are queued for merging when clean | **Settings, Automation**: Merge on its own, "Dependabot pull requests" | The merge still waits for your approval unless you change the policy below. |
| Merges run without your approval | **Policies**: `repo.merge_pull_request` set to automatic | The only switch that removes you from a merge. Leave it on approval until you trust the rest. |

## 7. The loop

1. **Connect a repository** (Connect repository). SDI scans it and shows
   what it understood: stack, pipelines, dependencies, maturity.
2. **Proposals** rank what to do next, with evidence and generated files
   (Dockerfile, workflows, Dependabot config, infrastructure). "Open pull
   request" puts an option's files on a branch and opens the pull request,
   after you approve the run.
3. **Pull requests** lists the repository's open pull requests, yours,
   your colleagues' and Dependabot's. Each gets a review: a walkthrough,
   and findings with the evidence and, where possible, the replacement
   code.
4. **Decide.** Accept or dismiss each finding. Some findings come with a
   "Suggested replacement", which is exactly what the lines should
   become. Others come with an "Example to adapt", which shows the shape
   of the fix and is never committed for you. "Draft a fix" asks the model
   for a replacement where there is none.
5. **Apply fixes** commits the accepted replacements to the pull request's
   branch as one commit, after approval. It never force-pushes, and it
   refuses pull requests from forks. The new commit is reviewed again.
6. **Merge** shows what stands in the way (failing checks, conflicts,
   undecided findings, branch protection) and, when nothing does, merges
   after approval.
7. **Security** (a project tab) lists vulnerable dependencies from
   Dependabot alerts, links each to the pull request that fixes it, and
   can turn Dependabot on for the repository. While alerts are off it
   falls back to an OSV lookup of the dependencies that name an exact
   version, and says so: a version range is not looked up, because that
   would report problems the project may not have.

Every action that changes the repository is a run under **Executions**
with its steps, logs and approvals, and every decision is in **Workspace,
Audit**.

## What it does not do

- It does not write features. The code it writes is generated delivery
  files (from what it learned about the repository) and fixes for the
  lines a review finding points at.
- It does not update lockfiles. Dependency upgrades are Dependabot's pull
  requests; SDI turns Dependabot on, reviews those pull requests and
  merges them.
- It does not override GitHub. Branch protection, required reviews and
  required checks are enforced by GitHub at merge time, and SDI stops and
  says so when they refuse.

## When something is off

```sh
make doctor
```

Then [`adapters.md`](adapters.md#troubleshooting) for a refused step, and
the **System** page in the app for the state of each service.
