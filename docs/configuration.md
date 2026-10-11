# Repository configuration: `.sdi.yml`

A repository can tell the platform what cannot be discovered by reading it:
where it should deploy, how much it may cost, how to test it. The file is
optional and lives at the repository root. The normative text is
[`CONTRACT.md` §12](../CONTRACT.md#12-repository-configuration--sdiyml);
this page is the user's guide to it.

## Why a file in the repository

The second principle of the platform is *discover first, ask only what
cannot be known*. A scan detects languages, frameworks, Dockerfiles and
workflows on its own, and what it cannot detect becomes an open `Question`
asked once in the UI. `.sdi.yml` is the way to answer those questions in
code: committed, reviewed, versioned with the repository, identical for every
person who connects it. Values from the file are **configured facts**
(`FACT_KIND_CONFIGURED`, confidence 1.0, `source_path=.sdi.yml`), a third
kind next to *detected* and *inferred*, and the UI shows them with their own
badge so nobody mistakes a declaration for an observation.

## Schema

```yaml
version: 1                      # required
deploy:
  target: vm | azure | aws | gcp | kubernetes | fly | render | none   # answers deployment_target
  environments: [staging, production]                                 # answers environments
  region: westeurope
budget:
  monthly_usd: 20                 # becomes the budget constraint on every proposal
test:
  command: "cargo test"           # used by the docker adapter's tests.run_local
  health_path: "/health"          # polled by container.run
  port: 8080
scan:
  exclude: ["docs/legacy/**", "vendor/**"]   # glob, applied to wanted_files
```

Every block is optional; `version: 1` is the only required key.

| Key | Type | What it does |
|---|---|---|
| `version` | `1` | Required. Any other value is a parse error and the file is ignored. |
| `deploy.target` | one of `vm`, `azure`, `aws`, `gcp`, `kubernetes`, `fly`, `render`, `none` | Answers the `deployment_target` question (`answered_by=config`) and becomes the configured fact the ranking's policy signal and the generators read. |
| `deploy.environments` | list of names | Answers the `environments` question; each name becomes an `environment:<name>` fact and graph node. |
| `deploy.region` | string | Default region for generated infrastructure (`infra/<cloud>/…`, Bicep, workflows). Negotiation can still override it per proposal. |
| `budget.monthly_usd` | number | The `budget_monthly_usd` constraint on every proposal for the project: options above it are kept but marked (`unmet_requirements` gets `budget:<n>`) and lose 0.25 on `overall`; the cheapest viable option is always in the top 5. |
| `test.command` | string | Run inside the built image by the `docker` adapter's `tests.run_local` step. |
| `test.health_path` | path | Polled by `container.run` until it answers 2xx. |
| `test.port` | integer | The port the container publishes and the health poll targets. |
| `scan.exclude` | list of globs | Removed from `wanted_files(tree)` before the scan downloads anything; use it for vendored code, generated files and legacy trees that would otherwise produce misleading facts. |

Unknown keys are reported in `ProjectConfig.errors` and ignored. A file that
does not parse is ignored entirely, and the error is reported the same way,
so a typo never silently changes a proposal.

## Worked example

A TypeScript web service that deploys to Azure, must stay under 20 USD a
month, and keeps a legacy tree the scanner should not read:

```yaml
version: 1
deploy:
  target: azure
  environments: [staging, production]
  region: westeurope
budget:
  monthly_usd: 20
test:
  command: "pnpm test"
  health_path: "/health"
  port: 3000
scan:
  exclude: ["docs/legacy/**", "fixtures/**"]
```

What happens on the next scan of that repository:

1. **Facts.** Four configured facts appear with confidence 1.0 and
   `source_path=.sdi.yml`: the deployment target, the two environments, the
   region, the budget. `GET /projects/{id}/facts?kind=configured` lists them.
   `Project.has_config` is true and `GET /projects/{id}/config` returns the
   parsed values (and `errors`, empty here).
2. **Questions.** The open questions `deployment_target` and `environments`
   are answered automatically with `answered_by=config`. They stay answered
   across rescans; delete the keys from the file and they reopen.
3. **Proposals.** Every proposal for the project carries the constraint
   `budget_monthly_usd=20`. An option estimated at 35 USD a month is still
   ranked, but with `budget:20` under its unmet requirements and 0.25 less on
   `overall`; the cheapest viable option is guaranteed a place in the top
   five even when its other signals are weak. Each option's `cost_breakdown`
   shows the lines (compute, database, registry, networking, CI minutes) that
   sum to its estimate, so the comparison against the budget is auditable.
4. **Policy signal.** `deploy.target: azure` acts like a recorded decision:
   patterns for other clouds score 0.0 on `policy`, Azure patterns 1.0, and
   cloud-neutral patterns stay neutral.
5. **Generated files.** `region: westeurope` and `port: 3000` are what the
   generators put into `infra/azure/main.bicep`, the deploy workflow and the
   Dockerfile's `EXPOSE`, instead of placeholders.
6. **Local verification.** The `docker` adapter runs `pnpm test` inside the
   built image and polls `http://…:3000/health` after `container.run`
   ([`adapters.md`](adapters.md)).
7. **Scan scope.** Nothing under `docs/legacy/` or `fixtures/` is downloaded,
   so an old `Dockerfile` kept there for reference no longer produces a
   `container:dockerfile` fact.

## Config versus negotiation versus decisions

Three things can push a proposal in a direction, and they are kept apart on
purpose:

| Source | Lives in | Scope | Changes |
|---|---|---|---|
| `.sdi.yml` | the repository | that project, every proposal | configured facts, answered questions, the budget constraint |
| Negotiation message | one proposal | that proposal (and its revisions) | constraints and weights |
| `Decision` | the platform | project or organization | the policy signal for every future ranking |

A negotiation turn such as "under $15 a month" sets the same
`budget_monthly_usd` constraint as the file would, but only on that proposal.
"We standardise on Azure" records an organization-wide `Decision`. The file
is the right place for what is true of the repository itself.

## What does not belong in the file

Anything that is a *document* rather than a *value*: README, architecture
notes, deployment guides, runbooks, ADRs, UAT plans. Those are generated by
the intelligence service, downloadable from the Documents page, and can be
committed to the repository through a `repo.open_pull_request` step. Keeping
`.sdi.yml` small is what keeps it reviewable.
