use std::{env, fs, path::Path, sync::{Arc, Mutex}};

use agent_ledger_agents::{get_adapter, AgentAdapter};
use agent_ledger_core::{
    event::{EventLog, EventType},
    session::{Session, SessionConfig, SessionId, SessionStatus},
    signing::SessionKey,
    storage::Storage,
    workspace::compute_workspace_hash,
};
use agent_ledger_runner::{git, process::ProcessRunner};
use chrono::Utc;
use serde_json::json;

use super::{capture_git_diff, event_log_path, ledger_dir, load_manifest_or_default, session_db_path, session_key_path, session_manifest_path, sessions_dir, write_session_manifest, EvidenceCapture, SessionManifestFile};

fn append_derived_agent_events(
    adapter: &dyn AgentAdapter,
    session_dir: &Path,
    event_log: &Arc<Mutex<EventLog>>,
) -> anyhow::Result<()> {
    let events = super::load_events(&event_log_path(session_dir))?;
    let mut derived_events = Vec::new();

    for event in events {
        if !matches!(event.event_type, EventType::AgentStdout | EventType::AgentStderr) {
            continue;
        }

        let Some(line) = event.payload.get("line").and_then(|value| value.as_str()) else {
            continue;
        };

        for parsed in adapter.parse_output_event(line) {
            match parsed.event_type.as_str() {
                "token_report" => derived_events.push((EventType::TokenReport, parsed.data)),
                "warning" => derived_events.push((EventType::Warning, parsed.data)),
                "error" => derived_events.push((EventType::Error, parsed.data)),
                _ => {}
            }
        }
    }

    if derived_events.is_empty() {
        return Ok(());
    }

    let mut log = event_log.lock().map_err(|_| anyhow::anyhow!("event log mutex poisoned"))?;
    for (event_type, payload) in derived_events {
        log.append(event_type, payload)?;
    }

    Ok(())
}

pub async fn run(agent: String) -> anyhow::Result<()> {
    let manifest = load_manifest_or_default()?;
    if !manifest.allowed_agents.iter().any(|allowed| allowed == &agent) {
        anyhow::bail!("agent '{agent}' is not allowed by ledger.yaml")
    }

    let adapter = get_adapter(&agent).ok_or_else(|| anyhow::anyhow!("unsupported agent: {agent}"))?;
    let detection = adapter.detect()?;
    if !detection.found {
        anyhow::bail!("agent '{}' is not available in PATH", agent)
    }
    let spec = adapter.launch_command(Path::new("."))?;
    let evidence_capture = EvidenceCapture::from_command_spec(&spec);

    let session_id = SessionId::new();
    let session_dir = sessions_dir().join(&session_id.0);
    fs::create_dir_all(session_dir.join("workspace.snapshots"))?;
    fs::create_dir_all(session_dir.join("diffs"))?;
    fs::create_dir_all(session_dir.join("test-results"))?;
    fs::create_dir_all(session_dir.join("final"))?;

    let storage = Storage::open(&session_db_path(&session_dir))?;
    let key = SessionKey::generate();
    let workspace_dir = env::current_dir()?;
    let baseline_workspace_hash = compute_workspace_hash(&workspace_dir)?;
    let baseline_commit = if git::is_git_repo(&workspace_dir) {
        Some(git::get_current_commit(&workspace_dir)?)
    } else {
        None
    };

    let mut session = Session::new(&SessionConfig {
        session_id: session_id.clone(),
        agent_name: agent.clone(),
        workspace_dir: workspace_dir.clone(),
        ledger_dir: ledger_dir(),
        manifest: manifest.clone(),
    });
    session.baseline_commit = baseline_commit.clone();
    session.baseline_workspace_hash = Some(baseline_workspace_hash.total_hash.clone());
    storage.save_session(&session)?;

    let session_manifest_file = SessionManifestFile {
        session: session.clone(),
        challenge_manifest: manifest.clone(),
        public_key_hex: key.public_key_hex(),
        evidence_capture: evidence_capture.clone(),
    };
    write_session_manifest(&session_manifest_path(&session_dir), &session_manifest_file)?;
    key.save_to_file(&session_key_path(&session_dir))?;

    let mut event_log = EventLog::new(&event_log_path(&session_dir), session_id.clone())?;
    event_log.append(
        EventType::SessionStarted,
        json!({
            "session_id": session_id.0,
            "agent": agent,
            "manifest_id": manifest.id,
            "baseline_commit": baseline_commit,
            "terminal_io_capture": evidence_capture.terminal_io,
            "capture_notes": evidence_capture.notes,
        }),
    )?;
    event_log.append(
        EventType::WorkspaceHashSnapshot,
        serde_json::to_value(&baseline_workspace_hash)?,
    )?;
    let baseline_diff = capture_git_diff(&workspace_dir);
    if let Some(payload) = baseline_diff.warning_payload("start") {
        event_log.append(EventType::Warning, payload)?;
    }
    if baseline_diff.file_contents().is_some() {
        event_log.append(EventType::GitDiffSnapshot, baseline_diff.event_payload())?;
    }

    let event_log = Arc::new(Mutex::new(event_log));
    let runner = ProcessRunner::new(session.id.to_string(), Arc::clone(&event_log));
    let exit_code = runner.run_agent(&spec, &workspace_dir).await?;
    append_derived_agent_events(adapter.as_ref(), &session_dir, &event_log)?;

    let final_workspace_hash = compute_workspace_hash(&workspace_dir)?;
    {
        let mut log = event_log.lock().map_err(|_| anyhow::anyhow!("event log mutex poisoned"))?;
        log.append(
            EventType::WorkspaceHashSnapshot,
            serde_json::to_value(&final_workspace_hash)?,
        )?;
    }

    session.final_workspace_hash = Some(final_workspace_hash.total_hash.clone());
    session.finished_at = Some(Utc::now());
    session.status = if exit_code == 0 {
        SessionStatus::Finished
    } else {
        SessionStatus::Failed
    };
    storage.save_session(&session)?;
    storage.update_session_status(&session.id, session.status.clone())?;

    {
        let mut log = event_log.lock().map_err(|_| anyhow::anyhow!("event log mutex poisoned"))?;
        log.append(
            EventType::SessionFinished,
            json!({
                "exit_code": exit_code,
                "status": session.status.to_string(),
                "final_workspace_hash": session.final_workspace_hash,
            }),
        )?;
    }

    write_session_manifest(
        &session_manifest_path(&session_dir),
        &SessionManifestFile {
            session,
            challenge_manifest: manifest,
            public_key_hex: key.public_key_hex(),
            evidence_capture,
        },
    )?;

    println!("Completed session {}", session_id);
    Ok(())
}
