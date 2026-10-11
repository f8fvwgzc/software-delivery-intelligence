//! Legacy and mixed codebases: Java EE and WAR packaging, .NET Framework
//! and Web Forms, plain PHP and WordPress, Python 2, classic ASP and JSP
//! pages, C/C++ build systems, Perl, and hand-rolled FTP deploys.
//!
//! Each signal is a Detected fact with the file that proves it, so the
//! maturity report and the modernization proposals can cite evidence
//! instead of "this looks old". Frameworks found here go through the same
//! `record` as the modern ones, so a Java EE app still gets a service node.

use regex::Regex;

use crate::detectors::Ctx;
use crate::detectors::frameworks::{Role, record};
use crate::files::is_deploy_script;
use crate::model::*;
use crate::util::{basename, depth, extension, line_with};

pub(crate) fn detect(ctx: &mut Ctx<'_>) {
    java(ctx);
    dotnet(ctx);
    php(ctx);
    python2(ctx);
    pages(ctx);
    build_systems(ctx);
    perl(ctx);
    deploy_scripts(ctx);
}

/// `index.jsp`, or `index.jsp and 3 more`.
fn sample_phrase(sample: &str, n: usize) -> String {
    match n {
        0 | 1 => sample.to_string(),
        _ => format!("{sample} and {} more", n - 1),
    }
}

fn count_ext(ctx: &Ctx<'_>, exts: &[&str]) -> (usize, String) {
    let mut n = 0;
    let mut sample = String::new();
    for p in ctx.tree_paths() {
        if exts.contains(&extension(p).as_str()) {
            n += 1;
            if sample.is_empty() {
                sample = p.to_string();
            }
        }
    }
    (n, sample)
}

// ---------------------------------------------------------------------------
// Java
// ---------------------------------------------------------------------------

fn is_java_ee_dep(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("javax.servlet")
        || n.starts_with("jakarta.servlet")
        || n.starts_with("javax:javaee")
        || n.starts_with("jakarta.platform:")
        || n.starts_with("javax.ejb")
        || n.starts_with("jakarta.ejb")
        || n.starts_with("javax.faces")
        || n.starts_with("jakarta.faces")
        || n.starts_with("javax.ws.rs")
        || n.starts_with("jakarta.ws.rs")
        || n.starts_with("javax.enterprise")
        || n.starts_with("org.glassfish:")
        || n.starts_with("org.jboss.spec")
        || n.starts_with("org.wildfly")
        || n == "javax:javaee-api"
}

fn java(ctx: &mut Ctx<'_>) {
    let war_pom = Regex::new(r"<packaging>\s*war\s*</packaging>").expect("static regex");
    let war_gradle =
        Regex::new(r#"(?m)^\s*(apply\s+plugin:\s*['"]war['"]|id\s*\(?\s*['"]war['"])"#)
            .expect("static regex");
    for m in ctx.manifests.clone() {
        if m.ecosystem != "maven" {
            continue;
        }
        let content = ctx.file(&m.path).unwrap_or("");
        let modern = m.deps.iter().any(|d| {
            d.name.contains("spring-boot")
                || d.name.starts_with("io.quarkus")
                || d.name.starts_with("io.micronaut")
                || d.name.starts_with("plugin:org.springframework.boot")
        });
        if let Some(cap) = war_pom.find(content).or_else(|| war_gradle.find(content))
            && !ctx.has_fact("packaging:war")
        {
            ctx.detected(
                FactCategory::Repository,
                "packaging:war",
                "WAR packaging",
                "application server deployment",
                &m.path,
                cap.as_str(),
            );
        }
        if let Some(d) = m.deps.iter().find(|d| is_java_ee_dep(&d.name))
            && !modern
        {
            let evidence = line_with(content, d.name.split(':').next_back().unwrap_or(&d.name))
                .map(String::from)
                .unwrap_or_else(|| format!("{} in {}", d.name, m.path));
            record(ctx, "java_ee", Role::Backend, &m.path, &evidence);
        }
        if let Some(d) = m
            .deps
            .iter()
            .find(|d| d.name.starts_with("org.apache.struts"))
        {
            let evidence = line_with(content, "struts")
                .map(String::from)
                .unwrap_or_else(|| format!("{} in {}", d.name, m.path));
            record(ctx, "struts", Role::Backend, &m.path, &evidence);
        }
    }
    // web.xml and JSP pages prove a servlet container even without a manifest.
    let web_xml = ctx
        .tree_paths()
        .find(|p| p.ends_with("WEB-INF/web.xml"))
        .map(String::from);
    if let Some(web_xml) = web_xml {
        let evidence = ctx
            .file(&web_xml)
            .and_then(|c| line_with(c, "<servlet").or_else(|| line_with(c, "<web-app")))
            .unwrap_or("web.xml in tree")
            .to_string();
        ctx.detected(
            FactCategory::Repository,
            "config:web_xml",
            "Servlet deployment descriptor",
            "WEB-INF/web.xml",
            &web_xml,
            &evidence,
        );
    }
    let (jsp, sample) = count_ext(ctx, &["jsp", "jspx"]);
    if jsp > 0 {
        ctx.detected(
            FactCategory::Repository,
            "pages:jsp",
            "JSP pages",
            &format!("{jsp} pages"),
            &sample,
            &sample_phrase(&sample, jsp),
        );
        if !ctx.has_fact("framework:java_ee") && !ctx.has_fact("framework:spring") {
            let manifest = ctx
                .manifests
                .iter()
                .find(|m| m.ecosystem == "maven")
                .map(|m| m.path.clone())
                .unwrap_or_default();
            record(
                ctx,
                "java_ee",
                Role::Backend,
                &manifest,
                &format!("{sample} (JSP)"),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// .NET
// ---------------------------------------------------------------------------

fn dotnet(ctx: &mut Ctx<'_>) {
    let tfm = Regex::new(r"<TargetFrameworks?>\s*([^<]+?)\s*</TargetFrameworks?>|<TargetFrameworkVersion>\s*([^<]+?)\s*</TargetFrameworkVersion>")
        .expect("static regex");
    let (aspx, aspx_sample) = count_ext(ctx, &["aspx", "ascx", "master"]);
    for m in ctx.manifests.clone() {
        if m.ecosystem != "nuget" || basename(&m.path) == "packages.config" {
            continue;
        }
        let content = ctx.file(&m.path).unwrap_or("");
        let mut framework_tfm: Option<String> = None;
        for cap in tfm.captures_iter(content) {
            let raw = cap
                .get(1)
                .or_else(|| cap.get(2))
                .map(|v| v.as_str())
                .unwrap_or("");
            for one in raw.split(';') {
                if let Some((rt, _)) = crate::eol::dotnet_tfm(one)
                    && rt == crate::eol::DOTNET_FRAMEWORK
                {
                    framework_tfm.get_or_insert_with(|| one.trim().to_string());
                }
            }
        }
        if let Some(t) = framework_tfm
            && !ctx.has_fact("dotnet:framework")
        {
            ctx.detected(
                FactCategory::Repository,
                "dotnet:framework",
                ".NET Framework",
                &t,
                &m.path,
                line_with(content, "TargetFramework").unwrap_or(&t),
            );
        }
        let core = content.contains("Microsoft.NET.Sdk.Web")
            || m.deps
                .iter()
                .any(|d| d.name.starts_with("Microsoft.AspNetCore"));
        let web_forms = m.has_dep("System.Web") && aspx > 0;
        let mvc =
            m.has_dep("System.Web.Mvc") || m.deps.iter().any(|d| d.name == "Microsoft.AspNet.Mvc");
        if core {
            let evidence = line_with(content, "Microsoft.NET.Sdk.Web")
                .or_else(|| line_with(content, "Microsoft.AspNetCore"))
                .unwrap_or("ASP.NET Core project")
                .to_string();
            record(ctx, "aspnet", Role::Backend, &m.path, &evidence);
        }
        if web_forms {
            record(
                ctx,
                "aspnet_webforms",
                Role::Backend,
                &m.path,
                &format!("System.Web reference and {aspx_sample}"),
            );
        }
        if mvc {
            let evidence = line_with(content, "System.Web.Mvc")
                .or_else(|| line_with(content, "Microsoft.AspNet.Mvc"))
                .unwrap_or("System.Web.Mvc")
                .to_string();
            record(ctx, "aspnet_mvc", Role::Backend, &m.path, &evidence);
        }
    }
    // packages.config and the project files it sits next to come from before PackageReference.
    if let Some(pc) = ctx
        .manifests
        .iter()
        .find(|m| basename(&m.path) == "packages.config")
        .map(|m| m.path.clone())
    {
        ctx.detected(
            FactCategory::Dependency,
            "dotnet:packages_config",
            "Legacy NuGet packages.config",
            "packages.config instead of PackageReference",
            &pc,
            line_with(ctx.file(&pc).unwrap_or(""), "<package ").unwrap_or("packages.config"),
        );
    }
    // A solution with project files we did not download still proves NuGet.
    let solution = ctx
        .tree_paths()
        .find(|p| p.ends_with(".sln") || p.ends_with(".csproj"))
        .map(String::from);
    if !ctx.has_fact("package:nuget")
        && let Some(p) = solution
    {
        ctx.detected(
            FactCategory::Dependency,
            "package:nuget",
            "NuGet",
            "solution in tree",
            &p,
            &format!("{p} present"),
        );
    }
    let web_config = ctx
        .tree_paths()
        .find(|p| basename(p).eq_ignore_ascii_case("web.config"))
        .map(String::from);
    if let Some(wc) = web_config {
        let evidence = ctx
            .file(&wc)
            .and_then(|c| {
                line_with(c, "targetFramework")
                    .or_else(|| line_with(c, "<appSettings"))
                    .or_else(|| line_with(c, "<connectionStrings"))
            })
            .unwrap_or("web.config in tree")
            .to_string();
        ctx.detected(
            FactCategory::Repository,
            "config:web_config",
            "IIS web.config",
            &wc,
            &wc,
            &evidence,
        );
        if ctx.has_fact("dotnet:framework") || aspx > 0 {
            ctx.deploy_targets.insert("iis".into());
        }
    }
    if aspx > 0 && !ctx.has_fact("framework:aspnet_webforms") && !ctx.has_fact("framework:aspnet") {
        record(
            ctx,
            "aspnet_webforms",
            Role::Backend,
            "",
            &format!("{} (Web Forms pages)", sample_phrase(&aspx_sample, aspx)),
        );
    }
}

// ---------------------------------------------------------------------------
// PHP
// ---------------------------------------------------------------------------

fn php(ctx: &mut Ctx<'_>) {
    let has_composer = ctx.manifests.iter().any(|m| m.ecosystem == "composer");
    let (php_files, sample) = count_ext(ctx, &["php"]);
    if php_files > 0 && !has_composer {
        ctx.detected(
            FactCategory::Repository,
            "php:plain",
            "Plain PHP without Composer",
            &format!("{php_files} *.php files"),
            &sample,
            "no composer.json",
        );
    }
    let wordpress = ctx.tree_has(|p| {
        let name = basename(p);
        name == "wp-config.php"
            || name == "wp-config-sample.php"
            || p.starts_with("wp-content/")
            || p.starts_with("wp-includes/")
            || p.contains("/wp-content/")
    });
    if wordpress && !ctx.has_fact("framework:wordpress") {
        let path = ctx
            .tree_paths()
            .find(|p| basename(p).starts_with("wp-config") || p.contains("wp-content"))
            .map(String::from)
            .unwrap_or_default();
        let manifest = ctx
            .manifests
            .iter()
            .find(|m| m.ecosystem == "composer")
            .map(|m| m.path.clone())
            .unwrap_or_default();
        record(
            ctx,
            "wordpress",
            Role::Fullstack,
            &manifest,
            &format!("{path} present"),
        );
    }
    if ctx.tree_has(|p| {
        p.ends_with("system/core/CodeIgniter.php") || p.ends_with("system/CodeIgniter.php")
    }) && !ctx.has_fact("framework:codeigniter")
    {
        record(
            ctx,
            "codeigniter",
            Role::Fullstack,
            "",
            "system/core/CodeIgniter.php present",
        );
    }
}

// ---------------------------------------------------------------------------
// Python 2
// ---------------------------------------------------------------------------

/// `==1.11.29`, `<2`, `<2.0`, `~=1.11` all pin Django to the 1.x line.
fn pins_django_1(spec: &str) -> bool {
    let s = spec.replace(' ', "");
    s.starts_with("==1.")
        || s.starts_with("~=1.")
        || s.split(',').any(|part| {
            part.starts_with("<2")
                || part.starts_with("<=1.")
                || part.starts_with("==1.")
                || part.starts_with("<1.")
        })
}

fn python2(ctx: &mut Ctx<'_>) {
    let mut signals: Vec<String> = Vec::new();
    let mut source = String::new();
    for m in ctx.manifests.clone() {
        if m.ecosystem != "pip" {
            continue;
        }
        if let Some(d) = m.dep("django")
            && pins_django_1(&d.version)
        {
            ctx.detected(
                FactCategory::Dependency,
                "dependency:eol:django",
                "End-of-life dependency major",
                &format!("Django {}", d.version),
                &m.path,
                line_with(ctx.file(&m.path).unwrap_or(""), "django").unwrap_or("django"),
            );
            signals.push(format!("Django {} in {}", d.version, m.path));
            if source.is_empty() {
                source = m.path.clone();
            }
        }
    }
    let has_modern_manifest = ctx.tree_has(|p| {
        depth(p) == 0 && matches!(basename(p), "pyproject.toml" | "Pipfile" | "setup.cfg")
            || (depth(p) == 0 && basename(p).starts_with("requirements"))
    });
    if ctx.tree_has(|p| p == "setup.py") && !has_modern_manifest {
        signals.push("setup.py without pyproject.toml or requirements".into());
        if source.is_empty() {
            source = "setup.py".into();
        }
    }
    let print_stmt = Regex::new(r#"(?m)^\s*print\s+["'\w]"#).expect("static regex");
    let except_comma = Regex::new(r"(?m)^\s*except\s+\w+\s*,\s*\w+\s*:").expect("static regex");
    for (path, content) in ctx.files_where(|p| extension(p) == "py") {
        if print_stmt.is_match(content) || except_comma.is_match(content) {
            signals.push(format!("Python 2 syntax in {path}"));
            if source.is_empty() {
                source = path.to_string();
            }
        }
    }
    for p in ["runtime.txt", ".python-version"] {
        if let Some(c) = ctx.file(p) {
            let v = c.trim().trim_start_matches("python-");
            if v.starts_with("2.") {
                signals.push(format!("{p}: {}", c.trim()));
                if source.is_empty() {
                    source = p.into();
                }
            }
        }
    }
    if signals.is_empty() {
        return;
    }
    signals.dedup();
    ctx.detected(
        FactCategory::Runtime,
        "python:2",
        "Python 2",
        "legacy runtime",
        &source,
        &signals.join("; "),
    );
}

// ---------------------------------------------------------------------------
// Page technologies that reveal the era
// ---------------------------------------------------------------------------

fn pages(ctx: &mut Ctx<'_>) {
    let (asp, sample) = count_ext(ctx, &["asp"]);
    if asp > 0 {
        ctx.detected(
            FactCategory::Repository,
            "pages:classic_asp",
            "Classic ASP pages",
            &format!("{asp} pages"),
            &sample,
            &sample_phrase(&sample, asp),
        );
    }
    let (aspx, sample) = count_ext(ctx, &["aspx", "ascx"]);
    if aspx > 0 {
        ctx.detected(
            FactCategory::Repository,
            "pages:aspx",
            "ASP.NET Web Forms pages",
            &format!("{aspx} pages"),
            &sample,
            &sample_phrase(&sample, aspx),
        );
    }
}

// ---------------------------------------------------------------------------
// C / C++ build systems
// ---------------------------------------------------------------------------

fn root_file(ctx: &Ctx<'_>, name: &str) -> Option<String> {
    ctx.tree_paths().find(|p| *p == name).map(String::from)
}

fn build_systems(ctx: &mut Ctx<'_>) {
    let cmake = root_file(ctx, "CMakeLists.txt");
    let autotools = root_file(ctx, "configure.ac")
        .or_else(|| root_file(ctx, "configure.in"))
        .or_else(|| root_file(ctx, "Makefile.am"));
    let makefile = root_file(ctx, "Makefile").or_else(|| root_file(ctx, "GNUmakefile"));
    if let Some(p) = cmake {
        let evidence = ctx
            .file(&p)
            .and_then(|c| {
                line_with(c, "project(").or_else(|| line_with(c, "cmake_minimum_required"))
            })
            .unwrap_or("CMakeLists.txt")
            .to_string();
        ctx.detected(
            FactCategory::Dependency,
            "build:cmake",
            "CMake build",
            "CMakeLists.txt",
            &p,
            &evidence,
        );
    }
    if let Some(p) = autotools {
        ctx.detected(
            FactCategory::Dependency,
            "build:autotools",
            "Autotools build",
            &p,
            &p,
            ctx.file(&p)
                .and_then(|c| line_with(c, "AC_INIT").or_else(|| line_with(c, "AM_INIT")))
                .unwrap_or("autotools files present"),
        );
    }
    let native = ctx.has_fact("lang:c") || ctx.has_fact("lang:cpp");
    if let Some(p) = makefile
        && (native || ctx.manifests.is_empty())
        && !ctx.has_fact("build:cmake")
        && !ctx.has_fact("build:autotools")
    {
        let evidence = ctx
            .file(&p)
            .and_then(|c| line_with(c, "all:").or_else(|| line_with(c, "build:")))
            .unwrap_or("Makefile")
            .to_string();
        ctx.detected(
            FactCategory::Dependency,
            "build:make",
            "Make build",
            &p,
            &p,
            &evidence,
        );
    }
}

// ---------------------------------------------------------------------------
// Perl
// ---------------------------------------------------------------------------

fn perl(ctx: &mut Ctx<'_>) {
    let (n, sample) = count_ext(ctx, &["pl", "pm"]);
    let has_build = ctx.tree_has(|p| matches!(p, "Makefile.PL" | "Build.PL" | "cpanfile"));
    if n == 0 && !has_build {
        return;
    }
    if !ctx.has_fact("lang:perl") {
        ctx.detected(
            FactCategory::Language,
            "lang:perl",
            "Perl",
            "secondary",
            &sample,
            &format!("{n} Perl files"),
        );
    }
    let build_file = ctx
        .tree_paths()
        .find(|p| matches!(*p, "Makefile.PL" | "Build.PL"))
        .map(String::from);
    if !ctx.has_fact("package:cpan")
        && let Some(p) = build_file
    {
        ctx.detected(
            FactCategory::Dependency,
            "package:cpan",
            "CPAN",
            &p,
            &p,
            &format!("{p} present"),
        );
    }
}

// ---------------------------------------------------------------------------
// Hand-rolled deploys
// ---------------------------------------------------------------------------

fn deploy_scripts(ctx: &mut Ctx<'_>) {
    let by_name = |p: &str| {
        let lower = basename(p).to_ascii_lowercase();
        lower == ".ftpconfig" || lower == "sftp-config.json" || lower.starts_with("ftp")
    };
    // Deploy scripts routinely inline credentials (`lftp -u user,pass`), so
    // the evidence names the command, never the line.
    let mut hit: Option<(String, String)> = None;
    for (path, content) in ctx.files_where(is_deploy_script) {
        let lower = content.to_ascii_lowercase();
        let needle = [
            "lftp",
            "ncftp",
            "sftp",
            "ftps://",
            "ftp://",
            "ftp ",
            "ftp\n",
            "curl -t",
            "curl --upload-file",
            "scp ",
        ]
        .into_iter()
        .find(|n| lower.contains(n))
        .map(|n| n.trim().to_string());
        if let Some(n) = needle {
            hit = Some((
                path.to_string(),
                format!("{n} command in {}", basename(path)),
            ));
            break;
        } else if by_name(path) {
            hit = Some((path.to_string(), format!("{} present", basename(path))));
            break;
        }
    }
    if hit.is_none()
        && let Some(p) = ctx.tree_paths().find(|p| by_name(p)).map(String::from)
    {
        hit = Some((p.clone(), format!("{p} present")));
    }
    if let Some((path, line)) = hit {
        ctx.detected(
            FactCategory::Deployment,
            "deploy:ftp_script",
            "FTP deploy script",
            &path,
            &path,
            &line,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn django_pins() {
        assert!(pins_django_1("==1.11.29"));
        assert!(pins_django_1("<2"));
        assert!(pins_django_1(">=1.8,<2.0"));
        assert!(pins_django_1("~=1.11"));
        assert!(!pins_django_1(">=4.2"));
        assert!(!pins_django_1("==5.0.1"));
        assert!(!pins_django_1(""));
    }
}
