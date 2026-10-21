//! The usage ledger in the window (K7): its rows, and the CSV export.

use crate::daemon::Client;
use crate::Res;
use nebula_core::studio::wire::LedgerRow;
use nebula_core::{ClientRequest, ProjectId, ServerEvent};
use std::sync::Arc;
use tauri::State;

/// The ledger's rows from `since` to `until` (local dates,
/// inclusive), for `repo` (a project id) or all.
#[tauri::command]
pub async fn usage_ledger(
    client: State<'_, Arc<Client>>,
    since: Option<String>,
    until: Option<String>,
    repo: Option<String>,
) -> Res<Vec<LedgerRow>> {
    rows(&client, since, until, repo).await
}

async fn rows(
    client: &Client,
    since: Option<String>,
    until: Option<String>,
    repo: Option<String>,
) -> Res<Vec<LedgerRow>> {
    let req_id = client.req_id();
    let req = ClientRequest::UsageLedger {
        req_id,
        since,
        until,
        project: repo.map(ProjectId),
    };
    match client.rpc_reply(req_id, req).await.map_err(|e| e.to_string())? {
        ServerEvent::UsageLedgerRows { rows, .. } => Ok(rows),
        _ => Err("the daemon answered the usage ledger with something else".into()),
    }
}

/// Write today's ledger to `<data dir>/exports/usage-<date>.csv`, show
/// the file in Explorer, and answer its path. The same file the terminal
/// app writes.
#[tauri::command]
pub async fn usage_export(app: tauri::AppHandle, client: State<'_, Arc<Client>>) -> Res<String> {
    use tauri_plugin_opener::OpenerExt;
    let day = nebula_core::usage_ledger::local_day(nebula_core::clock::now_secs() as i64);
    let rows = rows(&client, Some(day.clone()), Some(day.clone()), None).await?;
    let path = crate::editors::blocking(move || -> Res<std::path::PathBuf> {
        let dir = nebula_core::paths::data_dir().join("exports");
        std::fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        let path = dir.join(format!("usage-{day}.csv"));
        let mut out = std::fs::File::create(&path).map_err(|e| format!("could not write {}: {e}", path.display()))?;
        nebula_core::usage_ledger::write_csv(&rows, &mut out).map_err(|e| format!("could not write {}: {e}", path.display()))?;
        Ok(path)
    })
    .await??;
    // The file is written; a failed reveal is not a failed export.
    let _ = app.opener().reveal_item_in_dir(&path);
    Ok(path.display().to_string())
}
