use agent_ledger_core::{
    event::{Event, EventType},
    hash_chain::verify_chain,
};
use chrono::Utc;

use super::{active_or_latest_session_dir, event_log_path, latest_workspace_hash, load_events, read_session_manifest};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TokenTotals {
    reports: u64,
    reported_tokens_total: u64,
    input_tokens_total: u64,
    output_tokens_total: u64,
    cached_tokens_total: u64,
}

fn token_totals(events: &[Event]) -> TokenTotals {
    let mut totals = TokenTotals::default();

    for event in events {
        if event.event_type != EventType::TokenReport {
            continue;
        }

        totals.reports += 1;
        totals.reported_tokens_total += event
            .payload
            .get("reported_tokens_total")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        totals.input_tokens_total += event
            .payload
            .get("input_tokens")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        totals.output_tokens_total += event
            .payload
            .get("output_tokens")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        totals.cached_tokens_total += event
            .payload
            .get("cached_tokens")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
    }

    totals
}

pub async fn run() -> anyhow::Result<()> {
    let Some(session_dir) = active_or_latest_session_dir()? else {
        println!("No sessions found");
        return Ok(());
    };

    let session_manifest = read_session_manifest(&super::session_manifest_path(&session_dir))?;
    let events = load_events(&event_log_path(&session_dir))?;
    let chain_valid = verify_chain(&events).is_ok();
    let tokens = token_totals(&events);
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
    println!("token_reports: {}", tokens.reports);
    println!("reported_tokens_total: {}", tokens.reported_tokens_total);
    println!("reported_input_tokens_total: {}", tokens.input_tokens_total);
    println!("reported_output_tokens_total: {}", tokens.output_tokens_total);
    println!("reported_cached_tokens_total: {}", tokens.cached_tokens_total);
    println!("latest_workspace_hash: {}", latest_hash);
    println!("chain_valid: {}", chain_valid);
    Ok(())
}

#[cfg(test)]
mod tests {
    use agent_ledger_core::event::Event;
    use serde_json::json;

    use super::{token_totals, EventType};

    fn make_event(event_type: EventType, payload: serde_json::Value) -> Event {
        Event {
            seq: 0,
            timestamp: "2026-01-01T00:00:00Z".into(),
            session_id: "session-test".into(),
            event_type,
            payload,
            payload_hash: "h1".into(),
            prev_hash: "h0".into(),
            event_hash: "h2".into(),
        }
    }

    #[test]
    fn token_totals_aggregates_token_reports_only() {
        let events = vec![
            make_event(
                EventType::TokenReport,
                json!({
                    "reported_tokens_total": 42,
                    "input_tokens": 20,
                    "output_tokens": 15,
                    "cached_tokens": 7
                }),
            ),
            make_event(EventType::AgentStdout, json!({ "line": "ignored" })),
            make_event(
                EventType::TokenReport,
                json!({
                    "reported_tokens_total": 8,
                    "input_tokens": 2,
                    "output_tokens": 6
                }),
            ),
        ];

        let totals = token_totals(&events);

        assert_eq!(totals.reports, 2);
        assert_eq!(totals.reported_tokens_total, 50);
        assert_eq!(totals.input_tokens_total, 22);
        assert_eq!(totals.output_tokens_total, 21);
        assert_eq!(totals.cached_tokens_total, 7);
    }
}
