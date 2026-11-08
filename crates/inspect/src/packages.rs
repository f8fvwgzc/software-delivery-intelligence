//! Package databases and manifests an image carries: the distribution's
//! (`/etc/os-release`, dpkg, apk) and the application's (`node_modules`
//! `package.json`, Python `*.dist-info/METADATA`). Each parser takes the
//! file's text and returns what it names; nothing is guessed.

use crate::report::{Component, OsRelease};

/// `/etc/os-release` (`ID=debian`, `VERSION_ID="12"`).
pub fn os_release(text: &str) -> OsRelease {
    let mut out = OsRelease::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'').to_owned();
        match key.trim() {
            "ID" => out.id = value.to_ascii_lowercase(),
            "VERSION_ID" => out.version_id = value,
            "PRETTY_NAME" => out.pretty_name = value,
            _ => {}
        }
    }
    out
}

/// The OSV ecosystem of a distribution's packages: `Debian:12`,
/// `Alpine:v3.20`. Distributions OSV does not index this way get `None`
/// (their packages are listed, not looked up).
pub fn os_ecosystem(os: &OsRelease) -> Option<String> {
    match os.id.as_str() {
        "debian" => {
            let major = os.version_id.split('.').next().unwrap_or_default();
            (!major.is_empty()).then(|| format!("Debian:{major}"))
        }
        "alpine" => {
            let mut parts = os.version_id.split('.');
            match (parts.next(), parts.next()) {
                (Some(major), Some(minor)) => Some(format!("Alpine:v{major}.{minor}")),
                _ => None,
            }
        }
        _ => None,
    }
}

/// `/var/lib/dpkg/status` (or one file of distroless' `status.d/`):
/// paragraphs of `Package:`, `Version:`, `Status:`. Only installed ones.
pub fn dpkg(text: &str, ecosystem: &str, path: &str) -> Vec<Component> {
    let mut out = Vec::new();
    for paragraph in text.split("\n\n") {
        let field = |name: &str| {
            paragraph
                .lines()
                .find_map(|l| l.strip_prefix(name).map(|v| v.trim().to_owned()))
                .unwrap_or_default()
        };
        let name = field("Package:");
        let version = field("Version:");
        let status = field("Status:");
        let installed = status.is_empty() || status.ends_with(" installed");
        if !name.is_empty() && !version.is_empty() && installed {
            out.push(component(ecosystem, &name, &version, path, "dpkg"));
        }
    }
    out
}

/// `/lib/apk/db/installed`: `P:` name and `V:` version per paragraph.
pub fn apk(text: &str, ecosystem: &str, path: &str) -> Vec<Component> {
    let mut out = Vec::new();
    for paragraph in text.split("\n\n") {
        let field = |prefix: &str| {
            paragraph
                .lines()
                .find_map(|l| l.strip_prefix(prefix).map(str::to_owned))
                .unwrap_or_default()
        };
        let (name, version) = (field("P:"), field("V:"));
        if !name.is_empty() && !version.is_empty() {
            out.push(component(ecosystem, &name, &version, path, "apk"));
        }
    }
    out
}

/// A package's own `package.json` under `node_modules`.
pub fn npm(text: &str, path: &str) -> Option<Component> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let name = value["name"].as_str()?.trim();
    let version = value["version"].as_str()?.trim();
    (!name.is_empty() && !version.is_empty())
        .then(|| component("npm", name, version, path, "package.json"))
}

/// `METADATA` of an installed Python distribution.
pub fn python(text: &str, path: &str) -> Option<Component> {
    let mut name = "";
    let mut version = "";
    for line in text.lines() {
        if line.is_empty() {
            break; // headers end at the first blank line
        }
        if let Some(v) = line.strip_prefix("Name:") {
            name = v.trim();
        } else if let Some(v) = line.strip_prefix("Version:") {
            version = v.trim();
        }
    }
    (!name.is_empty() && !version.is_empty())
        .then(|| component("PyPI", name, version, path, "dist-info"))
}

/// The *names* of the variables a `.env` file sets. Values are never read
/// into the report.
pub fn env_names(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let l = l.strip_prefix("export ").unwrap_or(l);
            let (name, _) = l.split_once('=')?;
            let name = name.trim();
            (!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
                .then(|| name.to_owned())
        })
        .take(50)
        .collect()
}

/// A variable whose name says it holds a credential.
pub fn secret_like(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    [
        "SECRET",
        "TOKEN",
        "PASSWORD",
        "PASSWD",
        "PRIVATE",
        "CREDENTIAL",
        "API_KEY",
        "APIKEY",
        "ACCESS_KEY",
    ]
    .iter()
    .any(|word| upper.contains(word))
}

/// The kind of private key a PEM file holds, if it holds one.
pub fn private_key_kind(text: &str) -> Option<&'static str> {
    const MARKERS: [(&str, &str); 6] = [
        ("-----BEGIN RSA PRIVATE KEY-----", "RSA"),
        ("-----BEGIN EC PRIVATE KEY-----", "EC"),
        ("-----BEGIN DSA PRIVATE KEY-----", "DSA"),
        ("-----BEGIN OPENSSH PRIVATE KEY-----", "OpenSSH"),
        ("-----BEGIN ENCRYPTED PRIVATE KEY-----", "encrypted PKCS#8"),
        ("-----BEGIN PRIVATE KEY-----", "PKCS#8"),
    ];
    MARKERS
        .iter()
        .find(|(marker, _)| text.contains(marker))
        .map(|(_, kind)| *kind)
}

fn component(ecosystem: &str, name: &str, version: &str, path: &str, source: &str) -> Component {
    Component {
        ecosystem: ecosystem.to_owned(),
        name: name.to_owned(),
        version: version.to_owned(),
        path: path.to_owned(),
        source: source.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release_and_its_osv_ecosystem() {
        let debian = os_release(
            "PRETTY_NAME=\"Debian GNU/Linux 12 (bookworm)\"\nID=debian\nVERSION_ID=\"12\"\n",
        );
        assert_eq!(debian.id, "debian");
        assert_eq!(debian.version_id, "12");
        assert_eq!(os_ecosystem(&debian).as_deref(), Some("Debian:12"));
        let alpine = os_release("ID=alpine\nVERSION_ID=3.20.3\n");
        assert_eq!(os_ecosystem(&alpine).as_deref(), Some("Alpine:v3.20"));
        assert_eq!(
            os_ecosystem(&os_release("ID=ubuntu\nVERSION_ID=\"22.04\"")),
            None
        );
    }

    #[test]
    fn package_databases() {
        let status = "Package: openssl\nStatus: install ok installed\nVersion: 3.0.11-1~deb12u1\n\nPackage: gone\nStatus: deinstall ok config-files\nVersion: 1.0\n";
        let packages = dpkg(status, "Debian:12", "var/lib/dpkg/status");
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].name, "openssl");
        assert_eq!(packages[0].version, "3.0.11-1~deb12u1");
        let installed = "C:Q1abc=\nP:musl\nV:1.2.4-r2\n\nP:busybox\nV:1.36.1-r5\n";
        assert_eq!(
            apk(installed, "Alpine:v3.19", "lib/apk/db/installed").len(),
            2
        );
    }

    #[test]
    fn application_manifests() {
        let lodash = npm(
            r#"{"name":"lodash","version":"4.17.20"}"#,
            "app/node_modules/lodash/package.json",
        )
        .unwrap();
        assert_eq!(
            (lodash.ecosystem.as_str(), lodash.version.as_str()),
            ("npm", "4.17.20")
        );
        assert!(npm("{", "x").is_none());
        let flask = python(
            "Metadata-Version: 2.1\nName: Flask\nVersion: 2.0.1\n\nVersion: 9.9\n",
            "x",
        )
        .unwrap();
        assert_eq!(
            (flask.name.as_str(), flask.version.as_str()),
            ("Flask", "2.0.1")
        );
    }

    #[test]
    fn env_files_give_names_never_values() {
        let names =
            env_names("# db\nDATABASE_URL=postgres://u:p@h/db\nexport API_TOKEN='x'\nnot a line\n");
        assert_eq!(names, ["DATABASE_URL", "API_TOKEN"]);
        assert!(secret_like("API_TOKEN"));
        assert!(!secret_like("DATABASE_URL"));
        assert_eq!(
            private_key_kind("x\n-----BEGIN OPENSSH PRIVATE KEY-----\n"),
            Some("OpenSSH")
        );
        assert_eq!(private_key_kind("-----BEGIN CERTIFICATE-----"), None);
    }
}
