use std::{env, fs};

use agent_ledger_core::{event::{EventLog, EventType}, workspace::compute_workspace_hash};
use chrono::Utc;

use super::{active_or_latest_session_dir, capture_git_diff, event_log_path, read_session_manifest};

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

    let diff_capture = capture_git_diff(&workspace_dir);
    if let Some(diff_contents) = diff_capture.file_contents() {
        fs::write(
            session_dir.join("diffs").join(format!("{}.diff", Utc::now().timestamp())),
            diff_contents,
        )?;
    }

    let mut event_log = EventLog::new(&event_log_path(&session_dir), session_manifest.session.id.clone())?;
    event_log.append(EventType::WorkspaceHashSnapshot, serde_json::to_value(&workspace_hash)?)?;
    if let Some(payload) = diff_capture.warning_payload("snapshot") {
        event_log.append(EventType::Warning, payload)?;
    }
    if diff_capture.file_contents().is_some() {
        event_log.append(EventType::GitDiffSnapshot, diff_capture.event_payload())?;
    }

    println!("Snapshot captured for {}", session_manifest.session.id);
    Ok(())
}
