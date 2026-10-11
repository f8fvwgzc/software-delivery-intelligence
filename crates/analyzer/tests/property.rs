//! `analyze_at` and `wanted_files_with` must never panic, whatever the
//! repository contains. A deterministic xorshift generator (no dependency,
//! same cases on every run) throws random path sets, random text in every
//! file type the detectors parse, random exclude globs and random dates at
//! them. Failures print the seed so a case can be replayed.

use std::collections::BTreeMap;

use sdi_analyzer::*;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*: good enough to spread cases, trivially reproducible.
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

const PATHS: &[&str] = &[
    ".sdi.yml",
    "package.json",
    "Cargo.toml",
    "pyproject.toml",
    "requirements.txt",
    "setup.py",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "Gemfile",
    ".ruby-version",
    "composer.json",
    "mix.exs",
    "cpanfile",
    "Makefile.PL",
    "App.sln",
    "App/App.csproj",
    "App/packages.config",
    "App/Web.config",
    "global.json",
    ".nvmrc",
    ".node-version",
    ".tool-versions",
    "runtime.txt",
    ".python-version",
    ".java-version",
    ".sdkmanrc",
    "rust-toolchain.toml",
    "Dockerfile",
    "api/Dockerfile.prod",
    "docker-compose.yml",
    ".github/workflows/ci.yml",
    ".github/workflows/release.yml",
    ".gitlab-ci.yml",
    ".github/dependabot.yml",
    "infra/main.tf",
    "infra/app.bicep",
    "Pulumi.yaml",
    "Pulumi.prod.yaml",
    "k8s/deploy.yaml",
    "charts/app/Chart.yaml",
    "README.md",
    "docs/architecture.md",
    "docs/legacy/old.md",
    "migrations/0001_init.sql",
    ".env.example",
    ".env",
    "certs/server.pem",
    "src/main/webapp/WEB-INF/web.xml",
    "src/main/webapp/index.jsp",
    "legacy/default.asp",
    "web/Default.aspx",
    "wp-config.php",
    "index.php",
    "index.html",
    "app.py",
    "src/main.rs",
    "src/index.ts",
    "CMakeLists.txt",
    "configure.ac",
    "Makefile",
    "deploy.sh",
    ".ftpconfig",
    "vendor/x/package.json",
    "node_modules/y/package.json",
    "t/basic.t",
    "spec/thing_spec.rb",
    "tests/test_x.py",
    "a/b/c/d/e/f/g/h/main.go",
    "",
    "/",
    "..",
    "weird name with spaces.json",
    "ünïcödé/päth.yml",
];

const FRAGMENTS: &[&str] = &[
    "",
    "\n",
    "{",
    "}",
    "[",
    "]",
    ":",
    "version: 1\n",
    "version: \"one\"\n",
    "deploy:\n  target: azure\n",
    "deploy: 7\n",
    "scan:\n  exclude: [\"**\", \"*\", \"\"]\n",
    "budget:\n  monthly_usd: -1\n",
    "FROM ",
    "FROM node:16-alpine AS build\n",
    "FROM ${BASE}\n",
    "FROM :\n",
    "HEALTHCHECK\n",
    "USER\n",
    "<packaging>war</packaging>",
    "<maven.compiler.source>1.8</maven.compiler.source>",
    "<maven.compiler.source>${java.version}</maven.compiler.source>",
    "<TargetFramework>net48</TargetFramework>",
    "<TargetFrameworks>net6.0;netstandard2.0;net472</TargetFrameworks>",
    "<TargetFrameworkVersion>v</TargetFrameworkVersion>",
    "<PackageReference Include=\"\" />",
    "<Reference Include=\"System.Web\" />",
    "<package id=\"x\" />",
    "targetFramework=\"4.8\"",
    "go 1.21\n",
    "go \n",
    "ruby '2.6.6'\n",
    "ruby\n",
    "gem 'rails', '~> 5.2'\n",
    "requires 'Mojolicious';\n",
    "Django==1.11\n",
    "django<2\n",
    "print \"hi\"\n",
    "except Exception, e:\n",
    "[package]\nname = \"x\"\nedition = \"2021\"\nrust-version = \"1.85\"\n",
    "[package]\nedition = 2021\n",
    "{\"engines\":{\"node\":\">=16 <21\"}}",
    "{\"engines\":{\"node\":42}}",
    "{\"require\":{\"php\":\"^7.4\"}}",
    "{\"require\":{\"php\":[]}}",
    "{\"sdk\":{\"version\":\"8.0.100\"}}",
    "{\"sdk\":{\"version\":null}}",
    "python-2.7.18\n",
    "python-\n",
    "lts/gallium\n",
    "v16\n",
    "latest\n",
    "nodejs 16.20.0\npython 3.8.10\nruby 2.6.6\njava temurin-17.0.2\n",
    "nodejs\n",
    "java=17.0.2-tem\n",
    "[toolchain]\nchannel = \"1.85\"\n",
    "requires-python = \">=3.8\"\n[project]\nrequires-python = 3\n",
    "jobs:\n  release:\n    steps:\n      - run: cargo publish\n      - uses: softprops/action-gh-release@v2\n",
    "jobs:\n  deploy:\n    steps:\n      - uses: azure/webapps-deploy@v3\n",
    "on: [push]\njobs: 3\n",
    "services:\n  db:\n    image: postgres:16\n    healthcheck: {test: [\"CMD\", \"true\"]}\n  api:\n    image: node:14\n",
    "services: []\n",
    "lftp ftp://host\n",
    "scp build/ host:/var/www\n",
    "---\n---\n---\n",
    "\u{0}\u{1}\u{7f}",
    "ñandú 日本語 🦀",
    "\t\t\t",
    "a: &x [*x]\n",
    "{\"a\":\"",
    "<<<<<<<",
    "null",
    "true",
    "0",
    "-",
    "---\napiVersion: v1\nkind: \n",
    "resource \"aws_db_instance\" \"",
    "terraform { backend \"\" {} }",
];

const GLOBS: &[&str] = &[
    "",
    "*",
    "**",
    "**/",
    "/",
    ".",
    "..",
    "*/",
    "**/*.md",
    "docs/legacy/**",
    "vendor",
    "?",
    "???",
    "*.*",
    "a/*/*/**",
    "ünï*",
    "[",
    "\\",
    "**/**/**",
    "*.yml",
    ".sdi.yml",
];

fn text(rng: &mut Rng) -> String {
    let n = rng.below(8);
    let mut s = String::new();
    for _ in 0..n {
        s.push_str(rng.pick(FRAGMENTS));
        if rng.below(3) == 0 {
            s.push(char::from_u32(rng.below(0x3000) as u32 + 1).unwrap_or('x'));
        }
    }
    s
}

fn snapshot(rng: &mut Rng) -> RepoSnapshot {
    let n = rng.below(24);
    let mut tree: Vec<TreeEntry> = Vec::new();
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    for _ in 0..n {
        let path = rng.pick(PATHS).to_string();
        let content = text(rng);
        tree.push(TreeEntry {
            path: path.clone(),
            size: if rng.below(10) == 0 {
                u64::MAX
            } else {
                content.len() as u64
            },
            is_dir: rng.below(9) == 0,
        });
        if rng.below(4) != 0 {
            files.insert(path, content);
        }
    }
    let mut languages = BTreeMap::new();
    if rng.below(2) == 0 {
        languages.insert(
            rng.pick(&["Rust", "Perl", "C#", "HTML", "", "Zig"])
                .to_string(),
            rng.next(),
        );
    }
    RepoSnapshot {
        full_name: rng.pick(&["", "a/b", "/", "a/", "x//y"]).to_string(),
        description: text(rng),
        default_branch: "main".into(),
        commit_sha: String::new(),
        is_private: rng.below(2) == 0,
        primary_language: rng.pick(&["", "Rust", "Perl", "Zig"]).to_string(),
        languages,
        topics: vec![text(rng)],
        tree,
        files,
    }
}

#[test]
fn analyze_at_never_panics_on_garbage() {
    for seed in 1..=400u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let snap = snapshot(&mut rng);
        let today = (
            rng.below(5000) as i32 - 1000,
            rng.below(20) as u32,
            rng.below(40) as u32,
        );
        let a = std::panic::catch_unwind(|| analyze_at(&snap, today))
            .unwrap_or_else(|_| panic!("analyze_at panicked for seed {seed}"));
        assert!(!a.understanding.tldr.is_empty(), "seed {seed}");
        assert_eq!(a.maturity.axes.len(), 8, "seed {seed}");
        assert!(a.maturity.overall <= 100, "seed {seed}");
        let mut seen = std::collections::HashSet::new();
        for f in &a.facts {
            assert!(
                seen.insert(f.key.clone()),
                "seed {seed}: duplicate {}",
                f.key
            );
            assert!((0.0..=1.0).contains(&f.confidence), "seed {seed}");
        }
        let node_keys: std::collections::HashSet<&str> =
            a.nodes.iter().map(|n| n.key.as_str()).collect();
        for e in &a.edges {
            assert!(node_keys.contains(e.source_key.as_str()), "seed {seed}");
            assert!(node_keys.contains(e.target_key.as_str()), "seed {seed}");
        }
        let excludes: Vec<String> = (0..rng.below(5))
            .map(|_| rng.pick(GLOBS).to_string())
            .collect();
        let wanted = std::panic::catch_unwind(|| wanted_files_with(&snap.tree, &excludes))
            .unwrap_or_else(|_| panic!("wanted_files_with panicked for seed {seed}: {excludes:?}"));
        assert!(wanted.len() <= MAX_FILES, "seed {seed}");
        let _ = parse_config(&text(&mut rng));
    }
}

#[test]
fn extreme_dates_do_not_panic() {
    let snap = RepoSnapshot {
        files: BTreeMap::from([(".nvmrc".to_string(), "16".to_string())]),
        tree: vec![TreeEntry {
            path: ".nvmrc".into(),
            size: 2,
            is_dir: false,
        }],
        ..Default::default()
    };
    for today in [
        (i32::MIN, 0, 0),
        (i32::MAX, u32::MAX, u32::MAX),
        (0, 0, 0),
        (2023, 9, 11),
    ] {
        let a = analyze_at(&snap, today);
        assert!(a.facts.iter().any(|f| f.key == "runtime:node:16"));
    }
    let past = analyze_at(&snap, (2000, 1, 1));
    assert!(!past.facts.iter().any(|f| f.key == "runtime:eol:node"));
    let future = analyze_at(&snap, (2100, 1, 1));
    assert!(future.facts.iter().any(|f| f.key == "runtime:eol:node"));
}
