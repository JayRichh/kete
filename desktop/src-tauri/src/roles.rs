//! Role cards in the window (K3): the roles `repo` defines, for the role
//! pickers, and the guard that keeps a role off a daemon that cannot take
//! it.

use crate::daemon::Client;
use crate::Res;
use nebula_core::roles::{self, RolesFile};
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::State;

/// What the window says when a launch names a role and the daemon is older
/// than 66.
pub const TOO_OLD: &str = "this daemon is too old for roles: run kete kill and reopen";

/// Refuse a launch that names a role unless the connected daemon said
/// `STUDIO_PROTOCOL` or later at Hello: an older one drops the connection on
/// the longer frame.
pub fn require_roles(client: &Client) -> Res<()> {
    match client.daemon_protocol() {
        0 => Err("Not connected to the kete daemon.".into()),
        v if v < nebula_core::STUDIO_PROTOCOL => Err(TOO_OLD.into()),
        _ => Ok(()),
    }
}

/// A role id as the window sent it: trimmed, None when blank.
pub fn role_of(role: Option<String>) -> Option<String> {
    role.map(|r| r.trim().to_string()).filter(|r| !r.is_empty())
}

/// The rows the pickers show: one per role (the file's `[roles]` table is
/// a sorted map, so by id); none without a roles file.
pub fn rows(file: Option<RolesFile>) -> Value {
    Value::Array(
        file.map(|f| {
            f.roles
                .into_values()
                .map(|c| {
                    json!({
                        "id": c.id,
                        "label": c.label,
                        "model": c.model,
                        "permission": c.permission.as_str(),
                        "tier": c.tier,
                        "department": c.department,
                    })
                })
                .collect()
        })
        .unwrap_or_default(),
    )
}

/// The roles `repo`'s (a project id) committed `.kete/roles.toml` defines:
/// `[]` when it has none, an error when the file is invalid.
#[tauri::command]
pub async fn roles_list(client: State<'_, Arc<Client>>, repo: String) -> Res<Value> {
    let root = client
        .snapshot()
        .projects
        .iter()
        .find(|p| p.id.0 == repo)
        .map(|p| p.repo_path.clone())
        .ok_or("That repo is gone.")?;
    // `load_committed` runs git.
    crate::editors::blocking(move || roles::load_committed(&root).map(rows)).await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_carries_what_the_picker_shows() {
        let file = roles::parse(
            "version = 1\n\
             [roles.runner]\nlabel = \"Runner\"\nmodel = \"haiku\"\ntier = 3\npermission = \"allowlist\"\ndepartment = \"audio\"\n\
             [roles.lead]\n",
        )
        .unwrap();
        assert_eq!(
            rows(Some(file)),
            json!([
                {"id": "lead", "label": "lead", "model": null, "permission": "prompt", "tier": 2, "department": null},
                {"id": "runner", "label": "Runner", "model": "haiku", "permission": "allowlist", "tier": 3, "department": "audio"},
            ])
        );
        assert_eq!(rows(None), json!([]), "no roles file: no roles");
    }

    #[test]
    fn a_blank_role_is_none() {
        assert_eq!(role_of(Some(" runner ".into())).as_deref(), Some("runner"));
        assert_eq!(role_of(Some("  ".into())), None);
        assert_eq!(role_of(None), None);
    }

    #[test]
    fn a_role_never_reaches_an_old_daemon() {
        let (client, _rx) = Client::connected_for_test();
        client.set_model(|m| m.protocol_version = nebula_core::STUDIO_PROTOCOL - 1);
        assert_eq!(require_roles(&client).unwrap_err(), TOO_OLD);
        client.set_model(|m| m.protocol_version = nebula_core::STUDIO_PROTOCOL);
        assert!(require_roles(&client).is_ok());
    }
}
