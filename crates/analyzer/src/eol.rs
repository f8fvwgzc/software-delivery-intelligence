//! Curated end-of-life table and the version readers that feed it.
//!
//! Dates are constants on purpose: the analyzer has no clock and no network,
//! so "is this runtime still supported" is a pure function of the file
//! contents and the `today` the caller passes in. The rows follow
//! endoflife.date: the last day of free security updates from the project
//! itself (Node, Python, PHP, Ruby, Go, Ubuntu, Debian, Alpine), Microsoft's
//! published dates for .NET, and Oracle's end of premier support for Java.
//! Anything not in the table is "unknown", never "end of life".

/// A calendar day as `(year, month, day)`; tuples compare lexicographically,
/// which is exactly date order.
pub type Day = (i32, u32, u32);

/// Runtime keys as they appear in `runtime:<name>:<version>` fact keys.
pub const NODE: &str = "node";
pub const PYTHON: &str = "python";
pub const JAVA: &str = "java";
pub const DOTNET: &str = "dotnet";
pub const DOTNET_CORE: &str = "dotnet_core";
pub const DOTNET_FRAMEWORK: &str = "dotnet_framework";
pub const PHP: &str = "php";
pub const RUBY: &str = "ruby";
pub const GO: &str = "go";
pub const RUST: &str = "rust";
pub const UBUNTU: &str = "ubuntu";
pub const DEBIAN: &str = "debian";
pub const ALPINE: &str = "alpine";

/// `(runtime, version, end of life)`.
pub const EOL: &[(&str, &str, Day)] = &[
    // Node.js: even majors are LTS; odd ones are listed where they are common.
    (NODE, "10", (2021, 4, 30)),
    (NODE, "12", (2022, 4, 30)),
    (NODE, "14", (2023, 4, 30)),
    (NODE, "16", (2023, 9, 11)),
    (NODE, "18", (2025, 4, 30)),
    (NODE, "20", (2026, 4, 30)),
    (NODE, "22", (2027, 4, 30)),
    (NODE, "24", (2028, 4, 30)),
    // Python
    (PYTHON, "2.6", (2013, 10, 29)),
    (PYTHON, "2.7", (2020, 1, 1)),
    (PYTHON, "3.5", (2020, 9, 30)),
    (PYTHON, "3.6", (2021, 12, 23)),
    (PYTHON, "3.7", (2023, 6, 27)),
    (PYTHON, "3.8", (2024, 10, 7)),
    (PYTHON, "3.9", (2025, 10, 5)),
    (PYTHON, "3.10", (2026, 10, 4)),
    (PYTHON, "3.11", (2027, 10, 24)),
    (PYTHON, "3.12", (2028, 10, 2)),
    (PYTHON, "3.13", (2029, 10, 31)),
    // Java: Oracle end of premier support, which is when most teams treat an
    // LTS as legacy even though paid extended support continues.
    (JAVA, "6", (2015, 12, 31)),
    (JAVA, "7", (2019, 7, 31)),
    (JAVA, "8", (2022, 3, 31)),
    (JAVA, "11", (2023, 9, 30)),
    (JAVA, "17", (2026, 9, 30)),
    (JAVA, "21", (2028, 9, 30)),
    (JAVA, "25", (2030, 9, 30)),
    // .NET Framework: 4.6.2 and later follow the Windows lifecycle; they are
    // not end of life, so they are deliberately absent. The legacy signal for
    // them is the `dotnet:framework` fact, not an EOL date.
    (DOTNET_FRAMEWORK, "4.0", (2016, 1, 12)),
    (DOTNET_FRAMEWORK, "4.5", (2016, 1, 12)),
    (DOTNET_FRAMEWORK, "4.5.1", (2016, 1, 12)),
    (DOTNET_FRAMEWORK, "4.5.2", (2022, 4, 26)),
    (DOTNET_FRAMEWORK, "4.6", (2022, 4, 26)),
    (DOTNET_FRAMEWORK, "4.6.1", (2022, 4, 26)),
    // .NET Core
    (DOTNET_CORE, "1.0", (2019, 6, 27)),
    (DOTNET_CORE, "1.1", (2019, 6, 27)),
    (DOTNET_CORE, "2.0", (2018, 10, 1)),
    (DOTNET_CORE, "2.1", (2021, 8, 21)),
    (DOTNET_CORE, "2.2", (2019, 12, 23)),
    (DOTNET_CORE, "3.0", (2020, 3, 3)),
    (DOTNET_CORE, "3.1", (2022, 12, 13)),
    // .NET 5+
    (DOTNET, "5", (2022, 5, 10)),
    (DOTNET, "6", (2024, 11, 12)),
    (DOTNET, "7", (2024, 5, 14)),
    (DOTNET, "8", (2026, 11, 10)),
    (DOTNET, "9", (2026, 5, 12)),
    (DOTNET, "10", (2028, 11, 14)),
    // PHP: end of security support.
    (PHP, "5.6", (2018, 12, 31)),
    (PHP, "7.0", (2019, 1, 10)),
    (PHP, "7.1", (2019, 12, 1)),
    (PHP, "7.2", (2020, 11, 30)),
    (PHP, "7.3", (2021, 12, 6)),
    (PHP, "7.4", (2022, 11, 28)),
    (PHP, "8.0", (2023, 11, 26)),
    (PHP, "8.1", (2025, 12, 31)),
    (PHP, "8.2", (2026, 12, 31)),
    (PHP, "8.3", (2027, 12, 31)),
    (PHP, "8.4", (2028, 12, 31)),
    // Ruby
    (RUBY, "2.3", (2019, 3, 31)),
    (RUBY, "2.4", (2020, 4, 5)),
    (RUBY, "2.5", (2021, 3, 31)),
    (RUBY, "2.6", (2022, 4, 12)),
    (RUBY, "2.7", (2023, 3, 31)),
    (RUBY, "3.0", (2024, 4, 23)),
    (RUBY, "3.1", (2025, 3, 31)),
    (RUBY, "3.2", (2026, 3, 31)),
    (RUBY, "3.3", (2027, 3, 31)),
    (RUBY, "3.4", (2028, 3, 31)),
    // Go: a release is supported until two newer majors exist.
    (GO, "1.18", (2023, 2, 1)),
    (GO, "1.19", (2023, 8, 8)),
    (GO, "1.20", (2024, 2, 6)),
    (GO, "1.21", (2024, 8, 13)),
    (GO, "1.22", (2025, 2, 11)),
    (GO, "1.23", (2025, 8, 12)),
    (GO, "1.24", (2026, 2, 10)),
    (GO, "1.25", (2026, 8, 11)),
    (GO, "1.26", (2027, 2, 9)),
    // Ubuntu LTS: end of standard support.
    (UBUNTU, "16.04", (2021, 4, 30)),
    (UBUNTU, "18.04", (2023, 5, 31)),
    (UBUNTU, "20.04", (2025, 5, 31)),
    (UBUNTU, "22.04", (2027, 6, 1)),
    (UBUNTU, "24.04", (2029, 5, 31)),
    // Debian: end of LTS.
    (DEBIAN, "8", (2020, 6, 30)),
    (DEBIAN, "9", (2022, 6, 30)),
    (DEBIAN, "10", (2024, 6, 30)),
    (DEBIAN, "11", (2026, 8, 31)),
    (DEBIAN, "12", (2028, 6, 30)),
    (DEBIAN, "13", (2030, 6, 30)),
    // Alpine
    (ALPINE, "3.15", (2023, 11, 1)),
    (ALPINE, "3.16", (2024, 5, 23)),
    (ALPINE, "3.17", (2024, 11, 22)),
    (ALPINE, "3.18", (2025, 5, 9)),
    (ALPINE, "3.19", (2025, 11, 1)),
    (ALPINE, "3.20", (2026, 4, 1)),
    (ALPINE, "3.21", (2026, 11, 1)),
    (ALPINE, "3.22", (2027, 5, 1)),
];

/// The end-of-life day for a runtime version, if it is in the table.
pub fn eol_date(runtime: &str, version: &str) -> Option<Day> {
    EOL.iter()
        .find(|(r, v, _)| *r == runtime && *v == version)
        .map(|(_, _, d)| *d)
}

/// `Some(date)` when the runtime version is past its end of life on `today`.
pub fn is_eol(runtime: &str, version: &str, today: Day) -> Option<Day> {
    eol_date(runtime, version).filter(|d| *d <= today)
}

/// `2023-09-11`.
pub fn format_day(d: Day) -> String {
    format!("{:04}-{:02}-{:02}", d.0, d.1, d.2)
}

/// Marketing name of a runtime key.
pub fn runtime_name(runtime: &str) -> &str {
    match runtime {
        NODE => "Node",
        PYTHON => "Python",
        JAVA => "Java",
        DOTNET => ".NET",
        DOTNET_CORE => ".NET Core",
        DOTNET_FRAMEWORK => ".NET Framework",
        PHP => "PHP",
        RUBY => "Ruby",
        GO => "Go",
        RUST => "Rust",
        UBUNTU => "Ubuntu",
        DEBIAN => "Debian",
        ALPINE => "Alpine",
        other => other,
    }
}

/// `Node 16`, `Rust edition 2021`.
pub fn label(runtime: &str, version: &str) -> String {
    if runtime == RUST
        && let Some(ed) = version.strip_prefix("edition-")
    {
        return format!("Rust edition {ed}");
    }
    format!("{} {version}", runtime_name(runtime))
}

// ---------------------------------------------------------------------------
// Version normalisation. Each reader returns the version at the granularity
// the EOL table uses (Node major, Python major.minor, ...), or `None` when the
// text is not a version at all ("latest", "lts", "${VAR}").
// ---------------------------------------------------------------------------

/// Leading digits-and-dots run of a version-ish string: `v16.14.0-alpine` → `16.14.0`.
fn numeric_prefix(raw: &str) -> &str {
    let s = raw
        .trim()
        .trim_start_matches(['v', 'V', '=', '^', '~', '>', '<', ' '])
        .trim_start_matches("python-")
        .trim_start_matches("ruby-")
        .trim_start_matches("go");
    let end = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(s.len());
    s[..end].trim_end_matches('.')
}

fn component(raw: &str, n: usize) -> Option<String> {
    let num = numeric_prefix(raw);
    if num.is_empty() {
        return None;
    }
    let parts: Vec<&str> = num.split('.').filter(|p| !p.is_empty()).collect();
    if parts.len() < n {
        return None;
    }
    Some(parts[..n].join("."))
}

/// `16`, `v16.14.0`, `>=16`, `16.x`, `lts/gallium` → `16`.
pub fn node_major(raw: &str) -> Option<String> {
    let lower = raw.trim().to_ascii_lowercase();
    let codename = lower.strip_prefix("lts/").unwrap_or(&lower);
    let by_name = match codename {
        "argon" => Some("4"),
        "boron" => Some("6"),
        "carbon" => Some("8"),
        "dubnium" => Some("10"),
        "erbium" => Some("12"),
        "fermium" => Some("14"),
        "gallium" => Some("16"),
        "hydrogen" => Some("18"),
        "iron" => Some("20"),
        "jod" => Some("22"),
        "krypton" => Some("24"),
        _ => None,
    };
    if let Some(v) = by_name {
        return Some(v.to_string());
    }
    // A range keeps its lower bound: `>=18 <21` means the project still runs on 18.
    let first = lower
        .split([' ', '|', ','])
        .find(|s| s.chars().any(|c| c.is_ascii_digit()))?;
    component(first, 1)
}

/// `3.8.10`, `python-3.8`, `>=3.8,<4` → `3.8`.
pub fn python_minor(raw: &str) -> Option<String> {
    let first = raw
        .trim()
        .split([',', ' ', '|'])
        .find(|s| s.chars().any(|c| c.is_ascii_digit()))?;
    component(first, 2)
}

/// `1.8` → `8`, `17`, `JavaVersion.VERSION_1_8` → `8`, `temurin-17.0.2` → `17`.
pub fn java_major(raw: &str) -> Option<String> {
    let s = raw.trim();
    // Gradle spells it `JavaVersion.VERSION_1_8` / `VERSION_17`; everything
    // else is a vendor prefix and a version: `temurin-17.0.2`, `jdk 11`.
    let s = match s.find("VERSION_") {
        Some(i) => s[i + "VERSION_".len()..].replace('_', "."),
        None => s.rsplit(['-', ' ']).next().unwrap_or(s).to_string(),
    };
    let num = numeric_prefix(&s);
    let mut parts = num.split('.');
    let major = parts.next()?;
    if major == "1" {
        let minor = parts.next()?;
        return (!minor.is_empty()).then(|| minor.to_string());
    }
    (!major.is_empty()).then(|| major.to_string())
}

/// `.NET` target framework moniker → `(runtime, version)`.
/// `net48` → framework 4.8, `netcoreapp3.1` → core 3.1, `net6.0-windows` → .NET 6.
pub fn dotnet_tfm(raw: &str) -> Option<(&'static str, String)> {
    let t = raw.trim().to_ascii_lowercase();
    let t = t.split('-').next().unwrap_or(&t).to_string();
    if t.starts_with("netstandard") {
        return None;
    }
    if let Some(v) = t.strip_prefix("netcoreapp") {
        return component(v, 2).map(|v| (DOTNET_CORE, v));
    }
    if let Some(v) = t.strip_prefix("v") {
        // TargetFrameworkVersion in old-style projects: v4.8, v4.5.2
        return dotnet_framework_version(v);
    }
    let v = t.strip_prefix("net")?;
    if v.contains('.') {
        let major = component(v, 1)?;
        return match major.as_str() {
            "1" | "2" | "3" => component(v, 2).map(|v| (DOTNET_CORE, v)),
            _ => Some((DOTNET, major)),
        };
    }
    // net48, net472, net45: digits are 4.8, 4.7.2, 4.5
    if v.starts_with('4') && v.len() >= 2 && v.chars().all(|c| c.is_ascii_digit()) {
        let dotted: Vec<String> = v.chars().map(|c| c.to_string()).collect();
        return Some((DOTNET_FRAMEWORK, dotted.join(".")));
    }
    None
}

/// `4.8`, `4.7.2`, `4.5` → .NET Framework version as the table spells it.
pub fn dotnet_framework_version(raw: &str) -> Option<(&'static str, String)> {
    let num = numeric_prefix(raw);
    if !num.starts_with('4') {
        return None;
    }
    let parts: Vec<&str> = num.split('.').collect();
    let v = match parts.len() {
        1 => "4.0".to_string(),
        2 => num.to_string(),
        _ => {
            if parts[2] == "0" {
                parts[..2].join(".")
            } else {
                parts[..3].join(".")
            }
        }
    };
    Some((DOTNET_FRAMEWORK, v))
}

/// `^7.4`, `>=8.1`, `7.4.33` → `7.4`.
pub fn php_minor(raw: &str) -> Option<String> {
    let first = raw
        .trim()
        .split(['|', ' ', ','])
        .find(|s| s.chars().any(|c| c.is_ascii_digit()))?;
    component(first, 2)
}

/// `2.6.6`, `ruby-2.6.6`, `~> 3.2` → `2.6`.
pub fn ruby_minor(raw: &str) -> Option<String> {
    component(raw.trim().trim_start_matches("~>").trim(), 2)
}

/// `1.21`, `1.21.0`, `go1.21` → `1.21`.
pub fn go_minor(raw: &str) -> Option<String> {
    component(raw, 2)
}

/// Ubuntu release from a tag or codename: `20.04`, `focal` → `20.04`.
pub fn ubuntu_version(raw: &str) -> Option<String> {
    let t = raw.trim().to_ascii_lowercase();
    let t = t.split('-').next().unwrap_or(&t);
    let by_name = match t {
        "xenial" => Some("16.04"),
        "bionic" => Some("18.04"),
        "focal" => Some("20.04"),
        "jammy" => Some("22.04"),
        "noble" => Some("24.04"),
        _ => None,
    };
    by_name.map(String::from).or_else(|| component(t, 2))
}

/// Debian release from a tag or codename: `11`, `bullseye-slim` → `11`.
pub fn debian_version(raw: &str) -> Option<String> {
    let t = raw.trim().to_ascii_lowercase();
    let t = t.split('-').next().unwrap_or(&t);
    let by_name = match t {
        "jessie" => Some("8"),
        "stretch" => Some("9"),
        "buster" => Some("10"),
        "bullseye" => Some("11"),
        "bookworm" => Some("12"),
        "trixie" => Some("13"),
        _ => None,
    };
    by_name.map(String::from).or_else(|| component(t, 1))
}

/// `3.18`, `3.18.4` → `3.18`.
pub fn alpine_version(raw: &str) -> Option<String> {
    component(raw, 2)
}

/// Runtimes a container base image implies: `node:16-alpine3.18` is Node 16
/// on Alpine 3.18, `python:3.8-slim-bullseye` is Python 3.8 on Debian 11.
pub fn image_runtimes(image: &str) -> Vec<(&'static str, String)> {
    let image = image.trim().trim_matches('"').trim_matches('\'');
    let image = image.split('@').next().unwrap_or(image);
    let (name, tag) = match image.rfind(':') {
        Some(i) if !image[i + 1..].contains('/') => (&image[..i], &image[i + 1..]),
        _ => (image, ""),
    };
    let name = name.to_ascii_lowercase();
    let tag = tag.to_ascii_lowercase();
    let last = name.rsplit('/').next().unwrap_or(&name).to_string();
    let mut out: Vec<(&'static str, String)> = Vec::new();
    let mut push = |r: &'static str, v: Option<String>| {
        if let Some(v) = v
            && !out.iter().any(|(x, y)| *x == r && *y == v)
        {
            out.push((r, v));
        }
    };
    // Variant suffixes carry the distribution: -alpine3.18, -bullseye, -jammy.
    for part in tag.split('-').skip(1) {
        if let Some(v) = part.strip_prefix("alpine") {
            push(ALPINE, alpine_version(v));
        } else if let Some(v) = debian_codename(part) {
            push(DEBIAN, Some(v));
        } else if let Some(v) = ubuntu_codename(part) {
            push(UBUNTU, Some(v));
        }
    }
    if name.contains("mcr.microsoft.com/dotnet") || name.starts_with("dotnet/") {
        if name.contains("/framework/") {
            push(
                DOTNET_FRAMEWORK,
                dotnet_framework_version(&tag).map(|(_, v)| v),
            );
        } else if name.contains("/core/") {
            push(DOTNET_CORE, component(&tag, 2));
        } else if let Some(major) = component(&tag, 1) {
            match major.as_str() {
                "1" | "2" | "3" => push(DOTNET_CORE, component(&tag, 2)),
                _ => push(DOTNET, Some(major)),
            }
        }
        return out;
    }
    match last.as_str() {
        "node" => push(NODE, node_major(&tag)),
        "python" | "pypy" => push(PYTHON, python_minor(&tag)),
        "openjdk"
        | "eclipse-temurin"
        | "amazoncorretto"
        | "temurin"
        | "zulu"
        | "ibm-semeru-runtimes"
        | "sapmachine" => push(JAVA, java_major(tag.split('-').next().unwrap_or(""))),
        "maven" | "gradle" | "tomcat" | "jetty" => {
            // maven:3.9-eclipse-temurin-17, gradle:8.5-jdk17, tomcat:9-jdk8-openjdk:
            // the first component is the tool's own version, never the JDK.
            let parts: Vec<&str> = tag.split('-').collect();
            let digits = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit());
            let jdk = parts
                .iter()
                .find_map(|p| {
                    p.strip_prefix("jdk")
                        .or_else(|| p.strip_prefix("jre"))
                        .filter(|v| digits(v))
                })
                .or_else(|| parts.iter().skip(1).find(|p| digits(p)).copied());
            if let Some(j) = jdk {
                push(JAVA, java_major(j));
            }
        }
        "php" => push(PHP, php_minor(&tag)),
        "ruby" | "jruby" => push(RUBY, ruby_minor(&tag)),
        "golang" => push(GO, go_minor(&tag)),
        "rust" => push(RUST, component(&tag, 2)),
        "ubuntu" => push(UBUNTU, ubuntu_version(&tag)),
        "debian" => push(DEBIAN, debian_version(&tag)),
        "alpine" => push(ALPINE, alpine_version(&tag)),
        _ => {}
    }
    out
}

fn debian_codename(part: &str) -> Option<String> {
    matches!(
        part,
        "jessie" | "stretch" | "buster" | "bullseye" | "bookworm" | "trixie"
    )
    .then(|| debian_version(part))
    .flatten()
}

fn ubuntu_codename(part: &str) -> Option<String> {
    matches!(part, "xenial" | "bionic" | "focal" | "jammy" | "noble")
        .then(|| ubuntu_version(part))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_lookup_and_today() {
        assert_eq!(eol_date(NODE, "16"), Some((2023, 9, 11)));
        assert!(is_eol(NODE, "16", (2026, 10, 9)).is_some());
        assert!(is_eol(NODE, "22", (2026, 10, 9)).is_none());
        assert!(is_eol(NODE, "16", (2023, 9, 10)).is_none());
        assert!(is_eol(NODE, "16", (2023, 9, 11)).is_some());
        assert_eq!(eol_date("fortran", "77"), None);
        assert_eq!(format_day((2023, 9, 11)), "2023-09-11");
        assert_eq!(label(NODE, "16"), "Node 16");
        assert_eq!(label(RUST, "edition-2021"), "Rust edition 2021");
    }

    #[test]
    fn normalises_versions() {
        assert_eq!(node_major("v16.14.0").as_deref(), Some("16"));
        assert_eq!(node_major("lts/gallium").as_deref(), Some("16"));
        assert_eq!(node_major(">=18 <21").as_deref(), Some("18"));
        assert_eq!(node_major("16.x").as_deref(), Some("16"));
        assert_eq!(node_major("latest"), None);
        assert_eq!(python_minor("python-3.8.10").as_deref(), Some("3.8"));
        assert_eq!(python_minor(">=3.8,<4").as_deref(), Some("3.8"));
        assert_eq!(python_minor("3"), None);
        assert_eq!(java_major("1.8").as_deref(), Some("8"));
        assert_eq!(java_major("17").as_deref(), Some("17"));
        assert_eq!(java_major("JavaVersion.VERSION_1_8").as_deref(), Some("8"));
        assert_eq!(java_major("JavaVersion.VERSION_17").as_deref(), Some("17"));
        assert_eq!(java_major("temurin-17.0.2").as_deref(), Some("17"));
        assert_eq!(dotnet_tfm("net48"), Some((DOTNET_FRAMEWORK, "4.8".into())));
        assert_eq!(
            dotnet_tfm("net472"),
            Some((DOTNET_FRAMEWORK, "4.7.2".into()))
        );
        assert_eq!(
            dotnet_tfm("netcoreapp3.1"),
            Some((DOTNET_CORE, "3.1".into()))
        );
        assert_eq!(dotnet_tfm("net6.0-windows"), Some((DOTNET, "6".into())));
        assert_eq!(dotnet_tfm("net8.0"), Some((DOTNET, "8".into())));
        assert_eq!(
            dotnet_tfm("v4.5.2"),
            Some((DOTNET_FRAMEWORK, "4.5.2".into()))
        );
        assert_eq!(dotnet_tfm("netstandard2.0"), None);
        assert_eq!(php_minor("^7.4").as_deref(), Some("7.4"));
        assert_eq!(ruby_minor("ruby-2.6.6").as_deref(), Some("2.6"));
        assert_eq!(go_minor("1.21.0").as_deref(), Some("1.21"));
        assert_eq!(ubuntu_version("focal").as_deref(), Some("20.04"));
        assert_eq!(debian_version("bullseye-slim").as_deref(), Some("11"));
    }

    #[test]
    fn reads_base_images() {
        assert_eq!(
            image_runtimes("node:16-alpine3.18"),
            vec![(ALPINE, "3.18".to_string()), (NODE, "16".to_string())]
        );
        assert_eq!(
            image_runtimes("python:3.8-slim-bullseye"),
            vec![(DEBIAN, "11".to_string()), (PYTHON, "3.8".to_string())]
        );
        assert_eq!(
            image_runtimes("openjdk:8-jdk-alpine"),
            vec![(JAVA, "8".to_string())]
        );
        assert_eq!(
            image_runtimes("maven:3.9-eclipse-temurin-17"),
            vec![(JAVA, "17".to_string())]
        );
        assert_eq!(
            image_runtimes("gradle:8.5-jdk17"),
            vec![(JAVA, "17".to_string())]
        );
        assert_eq!(
            image_runtimes("mcr.microsoft.com/dotnet/aspnet:6.0"),
            vec![(DOTNET, "6".to_string())]
        );
        assert_eq!(
            image_runtimes("mcr.microsoft.com/dotnet/core/aspnet:3.1"),
            vec![(DOTNET_CORE, "3.1".to_string())]
        );
        assert_eq!(
            image_runtimes("mcr.microsoft.com/dotnet/framework/aspnet:4.8"),
            vec![(DOTNET_FRAMEWORK, "4.8".to_string())]
        );
        assert_eq!(
            image_runtimes("php:7.4-apache"),
            vec![(PHP, "7.4".to_string())]
        );
        assert_eq!(
            image_runtimes("ubuntu:focal"),
            vec![(UBUNTU, "20.04".to_string())]
        );
        assert_eq!(
            image_runtimes("debian:bookworm-slim"),
            vec![(DEBIAN, "12".to_string())]
        );
        assert_eq!(image_runtimes("nginx:1.27-alpine"), Vec::new());
        assert_eq!(image_runtimes("node:lts"), Vec::new());
        assert_eq!(image_runtimes("${BASE}"), Vec::new());
    }
}
