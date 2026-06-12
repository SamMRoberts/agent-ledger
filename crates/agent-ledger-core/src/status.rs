use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::{
    event::{Event, EventLog, EventType},
    hash_chain::verify_chain,
    manifest::ChallengeManifest,
    session::{Session, SessionStatus},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceCapture {
    pub terminal_io: String,
    pub notes: Vec<String>,
}

impl Default for EvidenceCapture {
    fn default() -> Self {
        Self {
            terminal_io: "unspecified".into(),
            notes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionManifestFile {
    pub session: Session,
    pub challenge_manifest: ChallengeManifest,
    pub public_key_hex: String,
    #[serde(default)]
    pub evidence_capture: EvidenceCapture,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TokenTotals {
    pub reports: u64,
    pub reported_tokens_total: u64,
    pub input_tokens_total: u64,
    pub output_tokens_total: u64,
    pub cached_tokens_total: u64,
    pub aic_used: Option<f64>,
    pub aic_remaining: Option<f64>,
    pub aic_limit: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct StatusSnapshot {
    pub session_dir: PathBuf,
    pub session: Session,
    pub elapsed_seconds: i64,
    pub event_count: usize,
    pub tokens: TokenTotals,
    pub latest_workspace_hash: String,
    pub chain_valid: bool,
}

pub fn ledger_dir(root: &Path) -> PathBuf {
    root.join(".ledger")
}

pub fn sessions_dir(root: &Path) -> PathBuf {
    ledger_dir(root).join("sessions")
}

pub fn session_db_path(session_dir: &Path) -> PathBuf {
    session_dir.join("session.db")
}

pub fn event_log_path(session_dir: &Path) -> PathBuf {
    session_dir.join("events.jsonl")
}

pub fn session_manifest_path(session_dir: &Path) -> PathBuf {
    session_dir.join("session_manifest.json")
}

pub fn session_key_path(session_dir: &Path) -> PathBuf {
    session_dir.join("session.key")
}

pub fn write_session_manifest(path: &Path, manifest: &SessionManifestFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(manifest)?)?;
    Ok(())
}

pub fn read_session_manifest(path: &Path) -> Result<SessionManifestFile> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

pub fn list_session_dirs(root: &Path) -> Result<Vec<PathBuf>> {
    let base = sessions_dir(root);
    if !base.exists() {
        return Ok(Vec::new());
    }

    let mut dirs = fs::read_dir(base)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    dirs.sort();
    Ok(dirs)
}

pub fn active_or_latest_session_dir(root: &Path) -> Result<Option<PathBuf>> {
    let dirs = list_session_dirs(root)?;
    for dir in dirs.iter().rev() {
        let manifest_path = session_manifest_path(dir);
        if manifest_path.exists() {
            let manifest = read_session_manifest(&manifest_path)?;
            if manifest.session.status == SessionStatus::Active {
                return Ok(Some(dir.clone()));
            }
        }
    }
    Ok(dirs.into_iter().last())
}

pub fn latest_workspace_hash(events: &[Event]) -> Option<String> {
    events.iter().rev().find_map(|event| {
        if event.event_type == EventType::WorkspaceHashSnapshot {
            event
                .payload
                .get("total_hash")?
                .as_str()
                .map(ToOwned::to_owned)
        } else {
            None
        }
    })
}

pub fn load_events(path: &Path) -> Result<Vec<Event>> {
    EventLog::load_all(path)
}

pub fn token_totals(events: &[Event]) -> TokenTotals {
    let mut totals = TokenTotals::default();

    for event in events {
        if event.event_type != EventType::TokenReport {
            continue;
        }

        totals.reports += 1;
        let is_cumulative = event
            .payload
            .get("report_kind")
            .and_then(|value| value.as_str())
            == Some("cumulative");
        let reported_tokens = event
            .payload
            .get("reported_tokens_total")
            .and_then(|value| value.as_u64());
        let input_tokens = event
            .payload
            .get("input_tokens")
            .and_then(|value| value.as_u64());
        let output_tokens = event
            .payload
            .get("output_tokens")
            .and_then(|value| value.as_u64());
        let cached_tokens = event
            .payload
            .get("cached_tokens")
            .and_then(|value| value.as_u64());

        if is_cumulative {
            if let Some(value) = reported_tokens {
                totals.reported_tokens_total = value;
            }
            if let Some(value) = input_tokens {
                totals.input_tokens_total = value;
            }
            if let Some(value) = output_tokens {
                totals.output_tokens_total = value;
            }
            if let Some(value) = cached_tokens {
                totals.cached_tokens_total = value;
            }
        } else {
            totals.reported_tokens_total += reported_tokens.unwrap_or(0);
            totals.input_tokens_total += input_tokens.unwrap_or(0);
            totals.output_tokens_total += output_tokens.unwrap_or(0);
            totals.cached_tokens_total += cached_tokens.unwrap_or(0);
        }

        if let Some(value) = event
            .payload
            .get("aic_used")
            .and_then(|value| value.as_f64())
        {
            totals.aic_used = Some(value);
        }
        if let Some(value) = event
            .payload
            .get("aic_remaining")
            .and_then(|value| value.as_f64())
        {
            totals.aic_remaining = Some(value);
        }
        if let Some(value) = event
            .payload
            .get("aic_limit")
            .and_then(|value| value.as_f64())
        {
            totals.aic_limit = Some(value);
        }
    }

    totals
}

pub fn load_status_snapshot(root: &Path) -> Result<Option<StatusSnapshot>> {
    let Some(session_dir) = active_or_latest_session_dir(root)? else {
        return Ok(None);
    };

    let session_manifest = read_session_manifest(&session_manifest_path(&session_dir))?;
    let events = load_events(&event_log_path(&session_dir))?;
    let latest_hash = latest_workspace_hash(&events)
        .or_else(|| session_manifest.session.final_workspace_hash.clone())
        .or_else(|| session_manifest.session.baseline_workspace_hash.clone())
        .unwrap_or_else(|| "n/a".into());
    let end = session_manifest
        .session
        .finished_at
        .unwrap_or_else(Utc::now);
    let elapsed_seconds = (end - session_manifest.session.started_at).num_seconds();

    Ok(Some(StatusSnapshot {
        session_dir,
        session: session_manifest.session,
        elapsed_seconds,
        event_count: events.len(),
        tokens: token_totals(&events),
        latest_workspace_hash: latest_hash,
        chain_valid: verify_chain(&events).is_ok(),
    }))
}

pub fn required_file<'a>(
    files: &'a std::collections::HashMap<String, Vec<u8>>,
    name: &str,
) -> Result<&'a [u8]> {
    files
        .get(name)
        .map(Vec::as_slice)
        .ok_or_else(|| anyhow!("missing required bundle file: {name}"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::{Duration, Utc};
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::{
        event::Event,
        manifest::ChallengeManifest,
        session::{Session, SessionId},
    };

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

    fn unique_test_root() -> PathBuf {
        std::env::temp_dir().join(format!("agent-ledger-status-test-{}", Uuid::new_v4()))
    }

    fn manifest_with_status(status: SessionStatus) -> SessionManifestFile {
        let started_at = Utc::now() - Duration::minutes(5);
        SessionManifestFile {
            session: Session {
                id: SessionId::from(format!("session-{}", Uuid::new_v4())),
                agent_name: "copilot".into(),
                started_at,
                finished_at: (status != SessionStatus::Active)
                    .then_some(started_at + Duration::minutes(3)),
                baseline_commit: None,
                baseline_workspace_hash: Some("baseline-hash".into()),
                final_workspace_hash: (status != SessionStatus::Active)
                    .then_some("final-hash".into()),
                status,
            },
            challenge_manifest: ChallengeManifest::default_manifest(),
            public_key_hex: "abcd".into(),
            evidence_capture: EvidenceCapture::default(),
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
        assert_eq!(totals.aic_used, None);
    }

    #[test]
    fn token_totals_uses_latest_cumulative_aic_report() {
        let events = vec![
            make_event(
                EventType::TokenReport,
                json!({
                    "source": "copilot_aic_usage",
                    "report_kind": "cumulative",
                    "aic_used": 1.0,
                    "aic_remaining": 49.0,
                    "aic_limit": 50.0,
                    "reported_tokens_total": 100
                }),
            ),
            make_event(
                EventType::TokenReport,
                json!({
                    "source": "copilot_aic_usage",
                    "report_kind": "cumulative",
                    "aic_used": 1.5,
                    "aic_remaining": 48.5,
                    "aic_limit": 50.0,
                    "reported_tokens_total": 125
                }),
            ),
        ];

        let totals = token_totals(&events);

        assert_eq!(totals.reports, 2);
        assert_eq!(totals.reported_tokens_total, 125);
        assert_eq!(totals.aic_used, Some(1.5));
        assert_eq!(totals.aic_remaining, Some(48.5));
        assert_eq!(totals.aic_limit, Some(50.0));
    }

    #[test]
    fn active_or_latest_session_dir_prefers_active_session() {
        let root = unique_test_root();
        let older_dir = sessions_dir(&root).join("session-older");
        let newer_dir = sessions_dir(&root).join("session-newer");
        fs::create_dir_all(&older_dir).expect("create older session dir");
        fs::create_dir_all(&newer_dir).expect("create newer session dir");

        write_session_manifest(
            &session_manifest_path(&older_dir),
            &manifest_with_status(SessionStatus::Finished),
        )
        .expect("write finished manifest");
        write_session_manifest(
            &session_manifest_path(&newer_dir),
            &manifest_with_status(SessionStatus::Active),
        )
        .expect("write active manifest");

        let chosen = active_or_latest_session_dir(&root)
            .expect("resolve session dir")
            .expect("some session dir");

        assert_eq!(chosen, newer_dir);

        fs::remove_dir_all(root).expect("cleanup test root");
    }

    #[test]
    fn load_status_snapshot_falls_back_to_manifest_hash_when_no_events_exist() {
        let root = unique_test_root();
        let session_dir = sessions_dir(&root).join("session-solo");
        fs::create_dir_all(&session_dir).expect("create session dir");

        write_session_manifest(
            &session_manifest_path(&session_dir),
            &manifest_with_status(SessionStatus::Finished),
        )
        .expect("write manifest");
        fs::write(event_log_path(&session_dir), "").expect("write empty event log");

        let snapshot = load_status_snapshot(&root)
            .expect("load snapshot")
            .expect("snapshot should exist");

        assert_eq!(snapshot.event_count, 0);
        assert_eq!(snapshot.latest_workspace_hash, "final-hash");
        assert!(snapshot.chain_valid);

        fs::remove_dir_all(root).expect("cleanup test root");
    }
}
