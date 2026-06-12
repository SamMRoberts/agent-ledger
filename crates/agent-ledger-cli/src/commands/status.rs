use std::path::Path;

use agent_ledger_core::status::load_status_snapshot;

pub async fn run() -> anyhow::Result<()> {
    let Some(snapshot) = load_status_snapshot(Path::new("."))? else {
        println!("No sessions found");
        return Ok(());
    };

    println!("session_id: {}", snapshot.session.id);
    println!("agent: {}", snapshot.session.agent_name);
    println!("status: {}", snapshot.session.status);
    println!("elapsed_seconds: {}", snapshot.elapsed_seconds);
    println!("event_count: {}", snapshot.event_count);
    println!("token_reports: {}", snapshot.tokens.reports);
    println!("reported_tokens_total: {}", snapshot.tokens.reported_tokens_total);
    println!("reported_input_tokens_total: {}", snapshot.tokens.input_tokens_total);
    println!("reported_output_tokens_total: {}", snapshot.tokens.output_tokens_total);
    println!("reported_cached_tokens_total: {}", snapshot.tokens.cached_tokens_total);
    println!("latest_workspace_hash: {}", snapshot.latest_workspace_hash);
    println!("chain_valid: {}", snapshot.chain_valid);
    Ok(())
}
