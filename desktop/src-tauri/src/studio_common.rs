//! What the window needs to know about studio mode before any studio page
//! opens: which projects are studio repos (`studio_repos`), for the rail's
//! Studio and Gauntlet entries and the menus' studio rows.
//!
//! A repo is a studio repo when its root has `.kete/studio.toml` (the
//! manifest) or this machine's `config.local.json` has a `studio` entry
//! for it. Both are read off the window's thread, fresh each call.

use crate::daemon::Client;
use crate::Res;
use nebula_core::studio::{manifest, settings, StudioSettings};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

/// One project as the studio pages see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StudioRepo {
    /// The project's id.
    pub repo: String,
    pub name: String,
    /// Its root has `.kete/studio.toml`.
    pub manifest: bool,
    /// This machine has a `studio` entry for it: the queue runs.
    pub enabled: bool,
    /// The entry's autonomy, `A0` to `A3`; None while it has none.
    pub autonomy: Option<String>,
}

/// Every project, each with whether it has a manifest and a `studio`
/// entry. The window keeps the ones with either.
#[tauri::command]
pub async fn studio_repos(client: State<'_, Arc<Client>>) -> Res<Vec<StudioRepo>> {
    let projects: Vec<(String, String, PathBuf)> = client
        .model
        .lock()
        .map_err(|_| "the model is unavailable".to_string())?
        .projects
        .iter()
        .map(|p| (p.id.0.clone(), p.name.clone(), p.repo_path.clone()))
        .collect();
    crate::editors::blocking(move || repos_in(&projects, &settings::load_all())).await
}

/// [`studio_repos`] against the projects and studio entries given.
pub fn repos_in(
    projects: &[(String, String, PathBuf)],
    entries: &BTreeMap<PathBuf, StudioSettings>,
) -> Vec<StudioRepo> {
    projects
        .iter()
        .map(|(id, name, root)| {
            let key = settings::repo_key(root);
            let entry = entries
                .iter()
                .find(|(k, _)| settings::same_repo(&settings::repo_key(k), &key))
                .map(|(_, s)| s);
            StudioRepo {
                repo: id.clone(),
                name: name.clone(),
                manifest: root.join(manifest::MANIFEST_PATH).is_file(),
                enabled: entry.is_some(),
                autonomy: entry.map(|s| s.autonomy.as_str().to_string()),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::studio::Autonomy;

    #[test]
    fn a_repo_is_a_studio_repo_by_its_manifest_or_its_entry() {
        let dir = tempfile::tempdir().unwrap();
        let with = dir.path().join("with");
        let entered = dir.path().join("entered");
        let plain = dir.path().join("plain");
        for d in [&with, &entered, &plain] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::create_dir_all(with.join(".kete")).unwrap();
        std::fs::write(with.join(".kete").join("studio.toml"), "version = 1\n").unwrap();
        let entries = [(
            PathBuf::from(settings::repo_key(&entered)),
            StudioSettings {
                autonomy: Autonomy::A2,
                ..StudioSettings::default()
            },
        )]
        .into_iter()
        .collect();
        let rows = repos_in(
            &[
                ("a".into(), "with".into(), with),
                ("b".into(), "entered".into(), entered),
                ("c".into(), "plain".into(), plain),
            ],
            &entries,
        );
        assert_eq!(rows.len(), 3, "every project");
        assert!(rows[0].manifest && !rows[0].enabled && rows[0].autonomy.is_none());
        assert!(!rows[1].manifest && rows[1].enabled);
        assert_eq!(rows[1].autonomy.as_deref(), Some("A2"));
        assert!(!rows[2].manifest && !rows[2].enabled);
    }
}
