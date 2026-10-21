//! The studio manifest: `<repo>/.kete/studio.toml`.
//!
//! A repo opts into the studio pages by committing this file. It says where
//! the inbox, the department list, the verdict folders and the evidence
//! folders live. Every key but `version` is optional, and unknown keys are
//! ignored so a newer file still loads in an older Kete.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The manifest's path, relative to the repo root.
pub const MANIFEST_PATH: &str = ".kete/studio.toml";
/// A manifest over this many bytes is refused before it is parsed.
pub const MAX_MANIFEST_BYTES: u64 = 65_536;

/// What a repo says about its studio. All paths are repo-relative.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StudioManifest {
    /// The file format version. Only 1 is understood.
    pub version: u32,
    /// The inbox folder.
    #[serde(default)]
    pub inbox: Option<String>,
    /// A Markdown file whose first table lists the departments.
    #[serde(default)]
    pub departments: Option<String>,
    /// The verdict folder: `<signoffs>/<kind>/<id>.json`.
    #[serde(default)]
    pub signoffs: Option<String>,
    /// The folder a verdict's `evidence` field points into.
    #[serde(default)]
    pub evidence: Option<String>,
    /// A local review app the pages link into.
    #[serde(default)]
    pub studio_url: Option<String>,
    /// The `to:` values announced to the orchestrator.
    #[serde(default)]
    pub recipients: Vec<String>,
    /// Verdict folder to department slug.
    #[serde(default)]
    pub kinds: BTreeMap<String, String>,
    /// Verdict folder to a route on `studio_url`; `{id}` is replaced.
    #[serde(default)]
    pub links: BTreeMap<String, String>,
}

/// Reads `<repo_root>/.kete/studio.toml`. A missing file is `Ok(None)`.
pub fn load(repo_root: &Path) -> Result<Option<StudioManifest>, String> {
    let path = repo_root.join(MANIFEST_PATH);
    let meta = match std::fs::metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("could not read {MANIFEST_PATH}: {e}")),
    };
    if meta.len() > MAX_MANIFEST_BYTES {
        return Err(format!("{MANIFEST_PATH} is over 64 KiB"));
    }
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("could not read {MANIFEST_PATH}: {e}")),
    };
    let text = String::from_utf8(bytes)
        .map_err(|_| format!("{MANIFEST_PATH} is not valid UTF-8 text"))?;
    parse(&text).map(Some)
}

/// Parses manifest text. Refuses a missing `version` and any version but 1.
pub fn parse(text: &str) -> Result<StudioManifest, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    match toml::from_str::<StudioManifest>(text) {
        Ok(manifest) => {
            if manifest.version == 1 {
                Ok(manifest)
            } else {
                Err(format!(
                    "{MANIFEST_PATH} has version {}, but only version 1 is understood",
                    manifest.version
                ))
            }
        }
        Err(e) => {
            let message = e.message();
            if message.contains("missing field `version`") {
                return Err(format!("{MANIFEST_PATH} needs `version = 1`"));
            }
            match e.span() {
                Some(span) => {
                    let end = span.start.min(text.len());
                    let line = 1 + text.as_bytes()[..end].iter().filter(|b| **b == b'\n').count();
                    Err(format!("{MANIFEST_PATH} line {line}: {message}"))
                }
                None => Err(format!("{MANIFEST_PATH}: {message}")),
            }
        }
    }
}

/// A department's slug: lowercase, each run of characters other than a-z
/// and 0-9 becomes one `-`, and no `-` at either end.
pub fn slug(label: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in label.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            if gap && !out.is_empty() {
                out.push('-');
            }
            gap = false;
            out.push(c);
        } else {
            gap = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"
version = 1
inbox = "Design/Studio/inbox"
departments = "Design/Production/DEPARTMENTS.md"
signoffs = "Design/Reviews/signoffs"
evidence = "Design/Reviews/evidence"
studio_url = "http://127.0.0.1:5173"
recipients = ["orchestrator", "manager"]
future_key = "ignored"

[kinds]
art = "art-and-3d"
sound = "audio"

[links]
art = "/art"
sound = "/sound?id={id}"
"#;

    fn write_manifest(root: &Path, bytes: &[u8]) {
        let dir = root.join(".kete");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("studio.toml"), bytes).unwrap();
    }

    #[test]
    fn every_key_round_trips() {
        let m = parse(FULL).unwrap();
        assert_eq!(m.version, 1);
        assert_eq!(m.inbox.as_deref(), Some("Design/Studio/inbox"));
        assert_eq!(m.departments.as_deref(), Some("Design/Production/DEPARTMENTS.md"));
        assert_eq!(m.signoffs.as_deref(), Some("Design/Reviews/signoffs"));
        assert_eq!(m.evidence.as_deref(), Some("Design/Reviews/evidence"));
        assert_eq!(m.studio_url.as_deref(), Some("http://127.0.0.1:5173"));
        assert_eq!(m.recipients, vec!["orchestrator", "manager"]);
        assert_eq!(m.kinds.get("art").map(String::as_str), Some("art-and-3d"));
        assert_eq!(m.kinds.len(), 2);
        assert_eq!(m.links.get("sound").map(String::as_str), Some("/sound?id={id}"));
        let again = parse(&toml::to_string(&m).unwrap()).unwrap();
        assert_eq!(again, m);
    }

    #[test]
    fn only_version_is_required() {
        let m = parse("version = 1\n").unwrap();
        assert_eq!(m, StudioManifest { version: 1, ..Default::default() });
    }

    #[test]
    fn missing_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()), Ok(None));
    }

    #[test]
    fn load_reads_the_file() {
        let dir = tempfile::tempdir().unwrap();
        write_manifest(dir.path(), FULL.as_bytes());
        let m = load(dir.path()).unwrap().unwrap();
        assert_eq!(m.inbox.as_deref(), Some("Design/Studio/inbox"));
    }

    #[test]
    fn a_bad_line_is_named() {
        let err = parse("version = 1\ninbox = \"a\"\nthis is not toml\n").unwrap_err();
        assert!(err.starts_with(".kete/studio.toml line 3:"), "{err}");
    }

    #[test]
    fn a_wrong_type_names_its_line() {
        let err = parse("version = 1\n\nrecipients = 4\n").unwrap_err();
        assert!(err.contains("line 3"), "{err}");
    }

    #[test]
    fn over_64_kib_is_refused_and_exactly_64_kib_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let mut text = String::from("version = 1\n# ");
        let mut ok = text.clone().into_bytes();
        ok.resize(MAX_MANIFEST_BYTES as usize - 1, b'x');
        ok.push(b'\n');
        assert_eq!(ok.len() as u64, MAX_MANIFEST_BYTES);
        write_manifest(dir.path(), &ok);
        assert!(load(dir.path()).unwrap().is_some());

        text.push_str(&"x".repeat(MAX_MANIFEST_BYTES as usize));
        let mut big = text.into_bytes();
        big.truncate(MAX_MANIFEST_BYTES as usize + 1);
        assert_eq!(big.len() as u64, MAX_MANIFEST_BYTES + 1);
        write_manifest(dir.path(), &big);
        let err = load(dir.path()).unwrap_err();
        assert!(err.contains("over 64 KiB"), "{err}");
    }

    #[test]
    fn version_two_is_refused() {
        let err = parse("version = 2\n").unwrap_err();
        assert!(err.contains("version 2"), "{err}");
    }

    #[test]
    fn missing_version_is_refused() {
        let err = parse("inbox = \"a\"\n").unwrap_err();
        assert!(err.contains("version"), "{err}");
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Art and 3D"), "art-and-3d");
        assert_eq!(slug("UX and UI"), "ux-and-ui");
        assert_eq!(slug("Web, community, release"), "web-community-release");
        assert_eq!(slug("  --Audio--  "), "audio");
        assert_eq!(slug(""), "");
    }
}
