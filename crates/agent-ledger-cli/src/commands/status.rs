use agent_ledger_core::hash_chain::verify_chain;
use chrono::Utc;

use super::{active_or_latest_session_dir, event_log_path, latest_workspace_hash, load_events, read_session_manifest};

pub async fn run() -> anyhow::Result<()> {
    let Some(session_dir) = active_or_latest_session_dir()? else {
        println!("No sessions found");
        return Ok(());
    };

    let session_manifest = read_session_manifest(&super::session_manifest_path(&session_dir))?;
    let events = load_events(&event_log_path(&session_dir))?;
    let chain_valid = verify_chain(&events).is_ok();
    let latest_hash = latest_workspace_hash(&events)
        .or_else(|| session_manifest.session.final_workspace_hash.clone())
        .or_else(|| session_manifest.session.baseline_workspace_hash.clone())
        .unwrap_or_else(|| "n/a".into());
    let end = session_manifest.session.finished_at.unwrap_or_else(Utc::now);
    let elapsed = end - session_manifest.session.started_at;

    println!("session_id: {}", session_manifest.session.id);
    println!("agent: {}", session_manifest.session.agent_name);
    println!("status: {}", session_manifest.session.status);
    println!("elapsed_seconds: {}", elapsed.num_seconds());
    println!("event_count: {}", events.len());
    println!("latest_workspace_hash: {}", latest_hash);
    println!("chain_valid: {}", chain_valid);
    Ok(())
}
