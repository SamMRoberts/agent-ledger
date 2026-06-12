use std::path::Path;

use agent_ledger_core::{session::SessionStatus, status::StatusSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconState {
    Idle,
    Active,
    Finished,
    Failed,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentedStatus {
    pub title: String,
    pub tooltip: String,
    pub session_label: String,
    pub detail_label: String,
    pub tokens_label: String,
    pub integrity_label: String,
    pub open_label: String,
    pub icon_state: IconState,
}

pub fn present_status(root: &Path, snapshot: Option<&StatusSnapshot>) -> PresentedStatus {
    match snapshot {
        Some(snapshot) => present_snapshot(snapshot),
        None => PresentedStatus {
            title: "AL idle".into(),
            tooltip: format!("No agent-ledger session found in {}", root.display()),
            session_label: "Session: none".into(),
            detail_label: "Run agent-ledger start to create a session".into(),
            tokens_label: "Tokens: none".into(),
            integrity_label: "Integrity: n/a".into(),
            open_label: "Open .ledger folder".into(),
            icon_state: IconState::Idle,
        },
    }
}

pub fn present_error(message: &str) -> PresentedStatus {
    PresentedStatus {
        title: "AL error".into(),
        tooltip: format!("agent-ledger menubar refresh failed\n{message}"),
        session_label: "Session: unavailable".into(),
        detail_label: "Status refresh failed".into(),
        tokens_label: "Tokens: unavailable".into(),
        integrity_label: "Integrity: error".into(),
        open_label: "Open workspace folder".into(),
        icon_state: IconState::Error,
    }
}

fn present_snapshot(snapshot: &StatusSnapshot) -> PresentedStatus {
    let status_label = match snapshot.session.status {
        SessionStatus::Active => "active",
        SessionStatus::Finished => "done",
        SessionStatus::Failed => "failed",
    };
    let icon_state = match snapshot.session.status {
        SessionStatus::Active => IconState::Active,
        SessionStatus::Finished => IconState::Finished,
        SessionStatus::Failed => IconState::Failed,
    };
    let total_tokens = snapshot.tokens.reported_tokens_total;
    let short_hash = short_hash(&snapshot.latest_workspace_hash);
    let chain_label = if snapshot.chain_valid {
        "valid"
    } else {
        "invalid"
    };

    PresentedStatus {
        title: format!("AL {status_label}"),
        tooltip: format!(
            "Session: {}\nAgent: {}\nStatus: {}\nElapsed: {}\nEvents: {}\nTokens: {}\nHash: {}\nChain: {}",
            snapshot.session.id,
            snapshot.session.agent_name,
            snapshot.session.status,
            format_elapsed(snapshot.elapsed_seconds),
            snapshot.event_count,
            total_tokens,
            short_hash,
            chain_label,
        ),
        session_label: format!("Session: {}", snapshot.session.id),
        detail_label: format!(
            "Agent: {} | Status: {} | Elapsed: {}",
            snapshot.session.agent_name,
            snapshot.session.status,
            format_elapsed(snapshot.elapsed_seconds),
        ),
        tokens_label: format!(
            "Tokens: {} reports | {} total | {} in / {} out",
            snapshot.tokens.reports,
            total_tokens,
            snapshot.tokens.input_tokens_total,
            snapshot.tokens.output_tokens_total,
        ),
        integrity_label: format!("Integrity: {chain_label} | Hash: {short_hash}"),
        open_label: "Open session folder".into(),
        icon_state,
    }
}

fn short_hash(hash: &str) -> String {
    hash.chars().take(8).collect()
}

fn format_elapsed(elapsed_seconds: i64) -> String {
    let total = elapsed_seconds.max(0);
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;

    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::{TimeZone, Utc};

    use super::*;
    use agent_ledger_core::{
        session::{Session, SessionId},
        status::TokenTotals,
    };

    fn snapshot(status: SessionStatus) -> StatusSnapshot {
        StatusSnapshot {
            session_dir: Path::new("/tmp/session").to_path_buf(),
            session: Session {
                id: SessionId::from("session-123"),
                agent_name: "copilot".into(),
                started_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
                finished_at: None,
                baseline_commit: None,
                baseline_workspace_hash: None,
                final_workspace_hash: None,
                status,
            },
            elapsed_seconds: 125,
            event_count: 7,
            tokens: TokenTotals {
                reports: 2,
                reported_tokens_total: 50,
                input_tokens_total: 20,
                output_tokens_total: 30,
                cached_tokens_total: 0,
                aic_used: None,
                aic_remaining: None,
                aic_limit: None,
            },
            latest_workspace_hash: "abcdef123456".into(),
            chain_valid: true,
        }
    }

    #[test]
    fn presents_idle_state_without_session() {
        let presented = present_status(Path::new("/repo"), None);

        assert_eq!(presented.title, "AL idle");
        assert_eq!(presented.icon_state, IconState::Idle);
        assert!(presented.tooltip.contains("/repo"));
    }

    #[test]
    fn presents_failed_session_state() {
        let presented = present_status(Path::new("/repo"), Some(&snapshot(SessionStatus::Failed)));

        assert_eq!(presented.title, "AL failed");
        assert_eq!(presented.icon_state, IconState::Failed);
        assert!(presented.detail_label.contains("failed"));
        assert!(presented.integrity_label.contains("abcdef12"));
    }
}
