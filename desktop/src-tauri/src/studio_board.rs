//! The studio board's window side (K4): the file-derived half of the
//! board (departments, notes, verdicts, refusals, the last renewal).
//!
//! Read-only (decision 11). Every call reads the repo's own files through
//! `nebula_core::studio::board`, off the window's thread, fresh each time.
//! The window joins the live half (sessions, roles, spend) from its store.

use crate::daemon::Client;
use crate::Res;
use nebula_core::studio::board::{self, BoardFiles};
use nebula_core::studio::inbox::InboxNote;
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;
use tauri::State;

/// What `studio_board` answers.
#[derive(Debug, Serialize)]
pub struct BoardReply {
    #[serde(flatten)]
    pub files: BoardFiles,
    /// The root checkout's worktree id, for opening a note in the file
    /// view; None when Kete has no worktree for it.
    pub worktree: Option<String>,
    /// `files.notes`, each with its repo-relative path. Flattened after
    /// `files`, so this `notes` is the one the window reads.
    pub notes: Vec<NoteRow>,
}

#[derive(Debug, Serialize)]
pub struct NoteRow {
    #[serde(flatten)]
    pub note: InboxNote,
    /// `path`, repo-relative and `/`-separated.
    pub rel: String,
}

/// [`studio_board`] for a repo at `root`.
pub fn board_at(root: &Path, worktree: Option<String>) -> BoardReply {
    let mut files = board::read_files(root);
    let notes = std::mem::take(&mut files.notes)
        .into_iter()
        .map(|note| NoteRow {
            rel: note
                .path
                .strip_prefix(root)
                .unwrap_or(&note.path)
                .to_string_lossy()
                .replace('\\', "/"),
            note,
        })
        .collect();
    BoardReply {
        files,
        worktree,
        notes,
    }
}

/// The board's file-derived half for `repo` (a project id).
#[tauri::command]
pub async fn studio_board(client: State<'_, Arc<Client>>, repo: String) -> Res<BoardReply> {
    let (root, worktree) = {
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
        (p.repo_path.clone(), worktree)
    };
    crate::editors::blocking(move || board_at(&root, worktree)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_carry_their_repo_relative_path() {
        let repo = tempfile::tempdir().unwrap();
        let r = repo.path();
        std::fs::create_dir_all(r.join(".kete")).unwrap();
        std::fs::write(
            r.join(".kete/studio.toml"),
            "version = 1\ninbox = \"notes/inbox\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(r.join("notes/inbox")).unwrap();
        std::fs::write(r.join("notes/inbox/n1.md"), "---\nto: audio\n---\nOne\n").unwrap();
        let reply = board_at(r, Some("w1".into()));
        assert_eq!(reply.notes.len(), 1);
        assert_eq!(reply.notes[0].rel, "notes/inbox/n1.md");
        let json = serde_json::to_value(&reply).unwrap();
        assert_eq!(json["notes"][0]["rel"], "notes/inbox/n1.md");
        assert_eq!(json["notes"][0]["id"], "n1");
        assert_eq!(json["worktree"], "w1");
        assert_eq!(json["manifest"], true);
    }
}
