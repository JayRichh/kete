//! The gauntlet view's window side (K5): verdicts by kind, an evidence
//! folder's listing, and opening the Studio app on a verdict.
//!
//! Read-only (decision 12). Every read is the repo's own files through
//! `nebula_core::studio::gauntlet`, off the window's thread, fresh each
//! call. The Studio link is built here from the manifest, never taken from
//! the page, and only a link to this machine opens.

use crate::daemon::Client;
use crate::Res;
use nebula_core::studio::gauntlet::{self, EvidenceEntry, Tone};
use nebula_core::studio::manifest;
use nebula_core::studio::verdicts::Verdict;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::State;

/// What `gauntlet_list` answers.
#[derive(Debug, Serialize)]
pub struct GauntletList {
    /// The repo's root on disk.
    pub root: String,
    /// The root checkout's worktree id, for Open in Kete and Show in
    /// Explorer; None when Kete has no worktree for it.
    pub worktree: Option<String>,
    /// Why there is nothing to show: no manifest, or one that does not
    /// parse.
    pub error: Option<String>,
    pub kinds: Vec<KindRow>,
}

#[derive(Debug, Serialize)]
pub struct KindRow {
    pub kind: String,
    pub department: Option<String>,
    /// Open in Studio applies to this kind's verdicts.
    pub studio: bool,
    pub verdicts: Vec<VerdictRow>,
}

#[derive(Debug, Serialize)]
pub struct VerdictRow {
    #[serde(flatten)]
    pub verdict: Verdict,
    pub tone: Tone,
    /// `path`, repo-relative.
    pub rel: String,
    /// `reconciled`, repo-relative.
    pub reconciled_rel: Option<String>,
}

/// The project's root and its main checkout's worktree id.
fn repo_of(client: &Client, repo: &str) -> Res<(PathBuf, Option<String>)> {
    let model = client
        .model
        .lock()
        .map_err(|_| "the model is unavailable".to_string())?;
    let p = model
        .projects
        .iter()
        .find(|p| p.id.0 == repo)
        .ok_or_else(|| "That repo is gone.".to_string())?;
    let worktree = model
        .worktrees
        .iter()
        .find(|w| w.project_id == p.id && w.is_main)
        .map(|w| w.id.0.clone());
    Ok((p.repo_path.clone(), worktree))
}

/// The manifest, or why there is none.
fn manifest_of(root: &Path) -> Res<manifest::StudioManifest> {
    manifest::load(root)?.ok_or_else(|| format!("This repo has no {}.", manifest::MANIFEST_PATH))
}

/// [`gauntlet_list`] for a repo at `root`.
pub fn list_at(root: &Path, worktree: Option<String>) -> GauntletList {
    let (kinds, error) = match manifest_of(root) {
        Ok(m) => {
            let kinds = gauntlet::list(root, &m)
                .into_iter()
                .map(|k| KindRow {
                    studio: gauntlet::studio_link(&m, &k.kind, "x").is_some(),
                    verdicts: k
                        .verdicts
                        .into_iter()
                        .map(|v| VerdictRow {
                            tone: gauntlet::tone(&v.verdict),
                            rel: gauntlet::repo_rel(root, &v.path),
                            reconciled_rel: v
                                .reconciled
                                .as_deref()
                                .map(|r| gauntlet::repo_rel(root, r)),
                            verdict: v,
                        })
                        .collect(),
                    kind: k.kind,
                    department: k.department,
                })
                .collect();
            (kinds, None)
        }
        Err(e) => (Vec::new(), Some(e)),
    };
    GauntletList {
        root: root.display().to_string(),
        worktree,
        error,
        kinds,
    }
}

/// Every verdict in `repo`'s sign-off folders, by kind.
#[tauri::command]
pub async fn gauntlet_list(client: State<'_, Arc<Client>>, repo: String) -> Res<GauntletList> {
    let (root, worktree) = repo_of(&client, &repo)?;
    crate::editors::blocking(move || list_at(&root, worktree)).await
}

/// The listing of one evidence folder `ev` in `repo`.
#[tauri::command]
pub async fn gauntlet_evidence(
    client: State<'_, Arc<Client>>,
    repo: String,
    ev: String,
) -> Res<Vec<EvidenceEntry>> {
    let (root, _) = repo_of(&client, &repo)?;
    crate::editors::blocking(move || {
        let m = manifest_of(&root)?;
        Ok(gauntlet::evidence(&root, &m, &ev))
    })
    .await?
}

/// The Studio link for verdict `id` of `kind` in the repo at `root`.
pub fn link_at(root: &Path, kind: &str, id: &str) -> Res<String> {
    let m = manifest_of(root)?;
    gauntlet::studio_link(&m, kind, id)
        .ok_or_else(|| "This verdict has no Studio link on this machine.".to_string())
}

/// Open the Studio app on verdict `id` of `kind`, in the system browser.
#[tauri::command]
pub async fn gauntlet_open_studio(
    app: tauri::AppHandle,
    client: State<'_, Arc<Client>>,
    repo: String,
    kind: String,
    id: String,
) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    let (root, _) = repo_of(&client, &repo)?;
    let url = crate::editors::blocking(move || link_at(&root, &kind, &id)).await??;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn a_repo_without_a_manifest_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let got = list_at(dir.path(), None);
        assert!(got.kinds.is_empty());
        assert!(got.error.unwrap().contains(".kete/studio.toml"));
        assert!(link_at(dir.path(), "art", "C02").is_err());
    }

    #[test]
    fn the_list_carries_tone_studio_and_repo_paths() {
        let dir = tempfile::tempdir().unwrap();
        put(
            dir.path(),
            ".kete/studio.toml",
            "version = 1\nsignoffs = \"s\"\nstudio_url = \"http://localhost:5173\"\n[links]\nart = \"/art?id={id}\"\n",
        );
        put(dir.path(), "s/art/C02.json", r#"{"verdict":"split"}"#);
        put(
            dir.path(),
            "s/art/C02-reconciled.json",
            r#"{"verdict":"approve"}"#,
        );
        put(dir.path(), "s/sound/tui.json", r#"{"verdict":"rejected"}"#);
        let got = list_at(dir.path(), Some("w1".into()));
        assert_eq!(got.error, None);
        assert_eq!(got.kinds.len(), 2);
        let art = &got.kinds[0];
        assert!(art.studio && !got.kinds[1].studio);
        let v = &art.verdicts[0];
        assert_eq!(v.tone, Tone::NeedsYou);
        assert_eq!(v.rel, "s/art/C02.json");
        assert_eq!(
            v.reconciled_rel.as_deref(),
            Some("s/art/C02-reconciled.json")
        );
        let json = serde_json::to_value(&got).unwrap();
        assert_eq!(json["kinds"][0]["verdicts"][0]["id"], "C02");
        assert_eq!(json["kinds"][0]["verdicts"][0]["tone"], "needs_you");
        assert_eq!(
            link_at(dir.path(), "art", "C02").unwrap(),
            "http://localhost:5173/art?id=C02"
        );
        assert!(link_at(dir.path(), "sound", "tui").is_err());
    }

    #[test]
    fn a_link_off_this_machine_never_opens() {
        let dir = tempfile::tempdir().unwrap();
        put(
            dir.path(),
            ".kete/studio.toml",
            "version = 1\nstudio_url = \"https://example.com\"\n[links]\nart = \"/art\"\n",
        );
        assert!(link_at(dir.path(), "art", "C02").is_err());
    }
}
