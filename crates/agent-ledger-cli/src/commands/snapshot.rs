use std::{env, fs};

use agent_ledger_core::{event::{EventLog, EventType}, workspace::compute_workspace_hash};
use agent_ledger_runner::git;
use chrono::Utc;
use serde_json::json;

use super::{active_or_latest_session_dir, event_log_path, read_session_manifest};

pub async fn run() -> anyhow::Result<()> {
    let session_dir = active_or_latest_session_dir()?.ok_or_else(|| anyhow::anyhow!("no sessions found"))?;
    let session_manifest = read_session_manifest(&super::session_manifest_path(&session_dir))?;
    let workspace_dir = env::current_dir()?;

    let workspace_hash = compute_workspace_hash(&workspace_dir)?;
    fs::write(
        session_dir
            .join("workspace.snapshots")
            .join(format!("{}.json", Utc::now().timestamp())),
        workspace_hash.to_json()?,
    )?;

    let diff = if git::is_git_repo(&workspace_dir) {
        git::get_diff(&workspace_dir).unwrap_or_default()
    } else {
        String::new()
    };
    fs::write(
        session_dir.join("diffs").join(format!("{}.diff", Utc::now().timestamp())),
        &diff,
    )?;

    let mut event_log = EventLog::new(&event_log_path(&session_dir), session_manifest.session.id.clone())?;
    event_log.append(EventType::WorkspaceHashSnapshot, serde_json::to_value(&workspace_hash)?)?;
    event_log.append(EventType::GitDiffSnapshot, json!({ "diff": diff }))?;

    println!("Snapshot captured for {}", session_manifest.session.id);
    Ok(())
}
