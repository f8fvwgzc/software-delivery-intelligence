//! Image references: what a release's `image`, `tag` and `digest` may look
//! like and how the defaults are derived (CONTRACT.md §22: the tag defaults
//! to the short commit SHA, the image to `<registry host>/<namespace>/<repo
//! name>`).

/// Characters of the short SHA a release is tagged with by default.
pub const SHORT_SHA_LEN: usize = 7;

/// The first seven characters of a commit SHA, lowercased.
pub fn short_sha(sha: &str) -> String {
    sha.trim()
        .chars()
        .take(SHORT_SHA_LEN)
        .collect::<String>()
        .to_ascii_lowercase()
}

/// 7 to 40 hex digits (64 for SHA-256 repositories).
pub fn valid_commit(sha: &str) -> bool {
    let len = sha.len();
    ((SHORT_SHA_LEN..=40).contains(&len) || len == 64) && sha.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The registry rule for tags: a word character, then up to 127 word
/// characters, dots or dashes.
pub fn valid_tag(tag: &str) -> bool {
    let bytes = tag.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && (bytes[0].is_ascii_alphanumeric() || bytes[0] == b'_')
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

/// `sha256:<64 hex>` (or `sha512:<128 hex>`).
pub fn valid_digest(digest: &str) -> bool {
    let Some((algorithm, hex)) = digest.split_once(':') else {
        return false;
    };
    let expected = match algorithm {
        "sha256" => 64,
        "sha512" => 128,
        _ => return false,
    };
    hex.len() == expected
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `host/namespace/name`: lowercase path components, an optional port on
/// the host, and neither a tag nor a digest.
pub fn valid_image(image: &str) -> bool {
    if image.is_empty() || image.len() > 255 || image.contains('@') || image.contains("//") {
        return false;
    }
    let mut components = image.split('/');
    let Some(first) = components.next() else {
        return false;
    };
    let component = |part: &str| {
        !part.is_empty()
            && part.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
            && part.bytes().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
            })
    };
    // Only the first component may carry a colon, and then it is a port.
    let host_ok = match first.rsplit_once(':') {
        Some((host, port)) => component(host) && port.parse::<u16>().is_ok(),
        None => component(first),
    };
    let rest: Vec<&str> = components.collect();
    host_ok && !rest.is_empty() && rest.iter().all(|part| component(part))
}

/// The repository name of `owner/name`.
pub fn repo_name(repo_full_name: &str) -> &str {
    repo_full_name
        .trim()
        .trim_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default()
}

/// A string as a path component of an image name: lowercase, anything a
/// registry would refuse replaced by a dash.
fn component(text: &str) -> String {
    let cleaned: String = text
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else if matches!(c, '.' | '_' | '-' | '/') {
                c
            } else {
                '-'
            }
        })
        .collect();
    cleaned.trim_matches(['-', '.', '_', '/']).to_owned()
}

/// `<registry host>/<namespace>/<repository name>`, lowercased (registries
/// refuse uppercase). An empty namespace is left out.
pub fn default_image(host: &str, namespace: &str, repo_full_name: &str) -> String {
    let namespace = component(namespace);
    let name = component(repo_name(repo_full_name));
    if namespace.is_empty() {
        format!("{}/{name}", host.trim().to_ascii_lowercase())
    } else {
        format!("{}/{namespace}/{name}", host.trim().to_ascii_lowercase())
    }
}

/// Splits `host/ns/name[:tag][@digest]` into `(image, tag, digest)`.
pub fn split_reference(reference: &str) -> (String, String, String) {
    let reference = reference.trim();
    let (rest, digest) = match reference.split_once('@') {
        Some((rest, digest)) => (rest, digest.to_owned()),
        None => (reference, String::new()),
    };
    match rest.rfind(':') {
        // A colon before the last slash is a registry port, not a tag.
        Some(colon) if !rest[colon..].contains('/') => (
            rest[..colon].to_owned(),
            rest[colon + 1..].to_owned(),
            digest,
        ),
        _ => (rest.to_owned(), String::new(), digest),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_the_contract() {
        assert_eq!(
            short_sha("A1B2C3D4E5F60718293A4B5C6D7E8F9012345678"),
            "a1b2c3d"
        );
        assert_eq!(short_sha("abc"), "abc");
        assert_eq!(
            default_image("ghcr.io", "Acme", "Acme/View-Engine"),
            "ghcr.io/acme/view-engine"
        );
        assert_eq!(
            default_image("myorg.azurecr.io", "team/platform", "acme/API Server"),
            "myorg.azurecr.io/team/platform/api-server"
        );
        assert_eq!(default_image("docker.io", "", "acme/api"), "docker.io/api");
        assert_eq!(repo_name("acme/api"), "api");
        assert_eq!(repo_name("api"), "api");
    }

    #[test]
    fn references_are_validated_and_split() {
        for good in ["a1b2c3d", "A1B2C3D4E5F60718293A4B5C6D7E8F9012345678"] {
            assert!(valid_commit(good), "{good}");
        }
        for bad in [
            "",
            "abc",
            "a1b2c3g",
            "a1b2c3d4e5f60718293a4b5c6d7e8f90123456789",
        ] {
            assert!(!valid_commit(bad), "{bad}");
        }
        for good in ["a1b2c3d", "v1.4.2", "latest", "_x", "1.0.0-rc.1"] {
            assert!(valid_tag(good), "{good}");
        }
        for bad in ["", ".x", "-x", "a b", "a:b", "a/b", &"x".repeat(129)] {
            assert!(!valid_tag(bad), "{bad}");
        }
        assert!(valid_digest(&format!("sha256:{}", "a".repeat(64))));
        for bad in [
            "",
            "sha256:abc",
            "md5:abc",
            &format!("sha256:{}", "G".repeat(64)),
            &"a".repeat(64),
        ] {
            assert!(!valid_digest(bad), "{bad}");
        }
        for good in [
            "ghcr.io/acme/api",
            "localhost:5000/acme/api",
            "docker.io/nginx",
            "a.b/c/d/e",
        ] {
            assert!(valid_image(good), "{good}");
        }
        for bad in [
            "",
            "api",
            "ghcr.io/Acme/api",
            "ghcr.io/acme/api:v1",
            "ghcr.io/acme/api@sha256:abc",
            "https://ghcr.io/acme/api",
            "ghcr.io//api",
            "ghcr.io/acme/ api",
        ] {
            assert!(!valid_image(bad), "{bad}");
        }
        assert_eq!(
            split_reference("ghcr.io/acme/api:v1"),
            ("ghcr.io/acme/api".into(), "v1".into(), String::new())
        );
        assert_eq!(
            split_reference("localhost:5000/acme/api"),
            (
                "localhost:5000/acme/api".into(),
                String::new(),
                String::new()
            )
        );
        assert_eq!(
            split_reference("ghcr.io/acme/api:v1@sha256:abc"),
            ("ghcr.io/acme/api".into(), "v1".into(), "sha256:abc".into())
        );
        assert_eq!(
            split_reference("ghcr.io/acme/api@sha256:abc"),
            (
                "ghcr.io/acme/api".into(),
                String::new(),
                "sha256:abc".into()
            )
        );
    }
}
