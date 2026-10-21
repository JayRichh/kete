//! The gauntlet view (K5): every verdict by kind, with its evidence and
//! the Studio app link, on top of `studio::verdicts`.
//!
//! Read-only (decision 12): verdict writes stay in the Studio app, and the
//! view links into it. Every path here comes from a committed file a cloned
//! repo controls, so each one is kept inside the repo ([`inside`]) and a
//! link only ever points at this machine ([`is_local`]).

use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use super::manifest::StudioManifest;
use super::verdicts::{self, Verdict};

/// An evidence folder lists at most this many entries.
pub const MAX_EVIDENCE_ENTRIES: usize = 200;

/// One kind folder's verdicts, newest first.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KindVerdicts {
    pub kind: String,
    /// The manifest's `[kinds]` slug for this folder.
    pub department: Option<String>,
    pub verdicts: Vec<Verdict>,
}

/// How a verdict word reads. Ember is for [`Tone::NeedsYou`] only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    Done,
    Failed,
    NeedsYou,
    /// Any other word, shown as written.
    Plain,
}

/// The tone of a verdict word, ignoring case and outer spaces.
pub fn tone(word: &str) -> Tone {
    match word.trim().to_ascii_lowercase().as_str() {
        "approve" | "approved" | "pass" => Tone::Done,
        "reject" | "rejected" | "regenerate" | "fail" => Tone::Failed,
        "split" | "third-reviewer" => Tone::NeedsYou,
        _ => Tone::Plain,
    }
}

/// The Studio app's page for verdict `id` of `kind`: `studio_url` plus
/// `links[kind]`, `{id}` replaced by the id, percent-encoded. None without
/// a `studio_url`, for a kind with no `[links]` route, or when the URL
/// would leave this machine.
pub fn studio_link(manifest: &StudioManifest, kind: &str, id: &str) -> Option<String> {
    let base = manifest.studio_url.as_deref()?.trim().trim_end_matches('/');
    let route = manifest.links.get(kind)?.trim();
    let route = route.replace("{id}", &encode(id));
    let url = if route.starts_with('/') {
        format!("{base}{route}")
    } else {
        format!("{base}/{route}")
    };
    is_local(&url).then_some(url)
}

/// Whether `url` is plain `http` on `127.0.0.1` or `localhost`, with at
/// most a numeric port: the authority is parsed, so `localhost@evil.com`
/// and `127.0.0.1.evil.com` are refused.
pub fn is_local(url: &str) -> bool {
    if url
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
    {
        return false;
    }
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (authority, None),
    };
    matches!(host, "127.0.0.1" | "localhost")
        && port.is_none_or(|p| (1..=5).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Every byte but A-Z, a-z, 0-9 and `-._~` as `%XX`.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `rel` under `root`, refused when it leaves it: only plain names (no
/// `..`, no root, no drive), and once symlinks are resolved it must still
/// be inside. The path given back is the plain joined one.
fn inside(root: &Path, rel: &str) -> Option<PathBuf> {
    let given = Path::new(rel);
    if rel.trim().is_empty()
        || given
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    let joined = root.join(given);
    let real_root = root.canonicalize().ok()?;
    let real = joined.canonicalize().ok()?;
    real.starts_with(&real_root).then_some(joined)
}

/// `path` relative to `root`, with `/` between names; the path as given
/// when it is not under `root`.
pub fn repo_rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Every kind folder under the manifest's `signoffs`, sorted, each with
/// its verdicts newest first. Empty without `signoffs`, or when it leaves
/// the repo.
pub fn list(repo_root: &Path, manifest: &StudioManifest) -> Vec<KindVerdicts> {
    let Some(signoffs) = manifest
        .signoffs
        .as_deref()
        .and_then(|s| inside(repo_root, s))
    else {
        return Vec::new();
    };
    verdicts::kinds(&signoffs)
        .into_iter()
        .map(|kind| KindVerdicts {
            department: manifest.kinds.get(&kind).cloned(),
            verdicts: verdicts::read_kind(&signoffs, &kind),
            kind,
        })
        .collect()
}

/// One file or folder of an evidence listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvidenceEntry {
    pub name: String,
    /// Repo-relative, `/` between names.
    pub path: String,
    pub is_dir: bool,
    /// The file's size; 0 for a folder.
    pub bytes: u64,
}

/// What evidence `ev_id` names under the manifest's `evidence` folder: a
/// folder's entries (folders first, then by name, at most
/// [`MAX_EVIDENCE_ENTRIES`]), or the one file it is. Symlinks are left
/// out. Empty when it is missing or would leave the repo.
pub fn evidence(repo_root: &Path, manifest: &StudioManifest, ev_id: &str) -> Vec<EvidenceEntry> {
    let Some(at) = manifest
        .evidence
        .as_deref()
        .and_then(|e| inside(repo_root, e))
        .and_then(|dir| inside(&dir, ev_id))
    else {
        return Vec::new();
    };
    let entry = |path: PathBuf, is_dir: bool, bytes: u64| EvidenceEntry {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: repo_rel(repo_root, &path),
        is_dir,
        bytes,
    };
    let Ok(meta) = std::fs::symlink_metadata(&at) else {
        return Vec::new();
    };
    if meta.is_file() {
        return vec![entry(at, false, meta.len())];
    }
    let Ok(read) = std::fs::read_dir(&at) else {
        return Vec::new();
    };
    let mut out: Vec<EvidenceEntry> = read
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let t = e.file_type().ok()?;
            if t.is_symlink() {
                return None;
            }
            let bytes = if t.is_file() {
                e.metadata().map(|m| m.len()).unwrap_or(0)
            } else {
                0
            };
            Some(entry(e.path(), t.is_dir(), bytes))
        })
        .take(MAX_EVIDENCE_ENTRIES)
        .collect();
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn manifest(text: &str) -> StudioManifest {
        super::super::manifest::parse(text).unwrap()
    }

    const STUDIO: &str = r#"
version = 1
signoffs = "reviews/signoffs"
evidence = "reviews/evidence"
studio_url = "http://127.0.0.1:5173/"
[kinds]
art = "art-and-3d"
[links]
art = "/art"
sound = "sound?id={id}"
"#;

    #[test]
    fn every_listed_word_has_its_tone() {
        for w in ["approve", "approved", "pass", " Approved "] {
            assert_eq!(tone(w), Tone::Done, "{w}");
        }
        for w in ["reject", "rejected", "regenerate", "fail", "REJECTED"] {
            assert_eq!(tone(w), Tone::Failed, "{w}");
        }
        for w in ["split", "third-reviewer"] {
            assert_eq!(tone(w), Tone::NeedsYou, "{w}");
        }
        for w in ["hold", "", "maybe-later"] {
            assert_eq!(tone(w), Tone::Plain, "{w}");
        }
        assert_eq!(
            serde_json::to_string(&Tone::NeedsYou).unwrap(),
            "\"needs_you\""
        );
    }

    #[test]
    fn studio_links() {
        let m = manifest(STUDIO);
        // No `{id}`: the route as written, and the trailing `/` trimmed.
        assert_eq!(
            studio_link(&m, "art", "C02").as_deref(),
            Some("http://127.0.0.1:5173/art")
        );
        // `{id}` replaced and encoded; a route without `/` gains one.
        assert_eq!(
            studio_link(&m, "sound", "tui call/2").as_deref(),
            Some("http://127.0.0.1:5173/sound?id=tui%20call%2F2")
        );
        // A kind with no route has no link.
        assert_eq!(studio_link(&m, "story", "s1"), None);
        // No studio_url, no link.
        let bare = manifest("version = 1\n[links]\nart = \"/art\"\n");
        assert_eq!(studio_link(&bare, "art", "C02"), None);
    }

    #[test]
    fn a_link_never_leaves_this_machine() {
        for url in [
            "http://127.0.0.1",
            "http://localhost:5173/art?id=1#x",
            "http://localhost/",
        ] {
            assert!(is_local(url), "{url}");
        }
        for url in [
            "https://127.0.0.1",
            "http://127.0.0.1.evil.com",
            "http://localhost@evil.com",
            "http://localhost@evil.com/",
            "http://127.0.0.1.evil.com:5173/",
            "http://evil.com/?localhost",
            "http://localhost:/",
            "http://localhost:123456",
            "http://localhost\\@evil.com",
            "http://localhost /x",
            "file:///C:/x",
            "javascript:alert(1)",
        ] {
            assert!(!is_local(url), "{url}");
        }
        let mut m = manifest(STUDIO);
        m.studio_url = Some("http://localhost@evil.com".into());
        assert_eq!(studio_link(&m, "art", "C02"), None);
    }

    #[test]
    fn list_groups_by_kind_and_a_reconciled_pair_once() {
        let dir = tempfile::tempdir().unwrap();
        let s = "reviews/signoffs";
        put(
            dir.path(),
            &format!("{s}/art/C02.json"),
            r#"{"verdict":"split","at":"2026-10-18"}"#,
        );
        put(
            dir.path(),
            &format!("{s}/art/C02-reconciled.json"),
            r#"{"verdict":"approve"}"#,
        );
        put(
            dir.path(),
            &format!("{s}/art/C05.json"),
            r#"{"verdict":"pass","at":"2026-10-20"}"#,
        );
        put(
            dir.path(),
            &format!("{s}/sound/tui.json"),
            r#"{"verdict":"rejected","reviewer":"Jay"}"#,
        );
        let got = list(dir.path(), &manifest(STUDIO));
        assert_eq!(
            got.iter().map(|k| k.kind.as_str()).collect::<Vec<_>>(),
            vec!["art", "sound"]
        );
        assert_eq!(got[0].department.as_deref(), Some("art-and-3d"));
        assert_eq!(got[1].department, None);
        let art: Vec<&str> = got[0].verdicts.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(art, vec!["C05", "C02"], "newest first, the pair once");
        let c02 = &got[0].verdicts[1];
        assert_eq!(
            repo_rel(dir.path(), c02.reconciled.as_deref().unwrap()),
            "reviews/signoffs/art/C02-reconciled.json"
        );
        assert_eq!(
            repo_rel(dir.path(), &c02.path),
            "reviews/signoffs/art/C02.json"
        );
    }

    #[test]
    fn list_refuses_a_signoffs_folder_outside_the_repo() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        put(dir.path(), "outside/art/a.json", "{}");
        std::fs::create_dir_all(&repo).unwrap();
        let outside = dir.path().join("outside");
        for s in ["../outside", outside.to_str().unwrap(), ""] {
            let mut m = manifest("version = 1\n");
            m.signoffs = Some(s.into());
            assert!(list(&repo, &m).is_empty(), "{s}");
        }
        assert!(list(&repo, &manifest("version = 1\n")).is_empty());
    }

    #[test]
    fn evidence_lists_a_folder_or_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let e = "reviews/evidence";
        put(dir.path(), &format!("{e}/EV1/b.png"), "12345");
        put(dir.path(), &format!("{e}/EV1/a.txt"), "x");
        std::fs::create_dir_all(dir.path().join(e).join("EV1/shots")).unwrap();
        put(dir.path(), &format!("{e}/EV2.md"), "hi");
        let m = manifest(STUDIO);
        let got = evidence(dir.path(), &m, "EV1");
        assert_eq!(
            got.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(),
            vec!["shots", "a.txt", "b.png"],
            "folders first, then by name"
        );
        assert!(got[0].is_dir && got[0].bytes == 0);
        assert_eq!(got[2].bytes, 5);
        assert_eq!(got[2].path, "reviews/evidence/EV1/b.png");
        let file = evidence(dir.path(), &m, "EV2.md");
        assert_eq!(file.len(), 1);
        assert_eq!(file[0].path, "reviews/evidence/EV2.md");
        assert!(evidence(dir.path(), &m, "EV9").is_empty());
    }

    #[test]
    fn evidence_refuses_a_path_out_of_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "reviews/evidence/EV1/a.txt", "x");
        put(dir.path(), "secret/key.txt", "x");
        let m = manifest(STUDIO);
        for ev in [
            "../../secret",
            "..",
            "EV1/../../../secret",
            "/etc",
            "C:\\Windows",
            "",
        ] {
            assert!(evidence(dir.path(), &m, ev).is_empty(), "{ev}");
        }
        let mut out = manifest(STUDIO);
        out.evidence = Some("../".into());
        assert!(evidence(dir.path(), &out, "EV1").is_empty());
    }

    #[test]
    fn evidence_stops_at_its_cap() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("reviews/evidence/EV1");
        std::fs::create_dir_all(&folder).unwrap();
        for i in 0..(MAX_EVIDENCE_ENTRIES + 5) {
            std::fs::write(folder.join(format!("f{i:03}.txt")), "x").unwrap();
        }
        let got = evidence(dir.path(), &manifest(STUDIO), "EV1");
        assert_eq!(got.len(), MAX_EVIDENCE_ENTRIES);
    }
}
