use std::{
    env, fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use agent_ledger_core::{
    event::{EventLog, EventType},
    manifest::ChallengeManifest,
    session::{Session, SessionConfig, SessionId, SessionStatus},
    signing::SessionKey,
    status::{
        event_log_path, ledger_dir, session_db_path, session_key_path, session_manifest_path,
        sessions_dir, EvidenceCapture, SessionManifestFile,
    },
    storage::Storage,
    workspace::{compute_workspace_hash, WorkspaceHash},
};
use agent_ledger_runner::git;
use chrono::Utc;
use serde_json::json;

use super::{capture_git_diff, write_session_manifest};

pub(crate) struct SessionContext {
    pub(crate) session: Session,
    pub(crate) session_dir: PathBuf,
    pub(crate) workspace_dir: PathBuf,
    pub(crate) event_log: Arc<Mutex<EventLog>>,
    manifest: ChallengeManifest,
    public_key_hex: String,
    evidence_capture: EvidenceCapture,
    storage: Storage,
}

impl SessionContext {
    pub(crate) fn for_existing(
        session_manifest: SessionManifestFile,
        session_dir: PathBuf,
    ) -> anyhow::Result<Self> {
        let workspace_dir = env::current_dir()?;
        let storage = Storage::open(&session_db_path(&session_dir))?;
        let event_log = EventLog::new(
            &event_log_path(&session_dir),
            session_manifest.session.id.clone(),
        )?;

        Ok(Self {
            session: session_manifest.session,
            session_dir,
            workspace_dir,
            event_log: Arc::new(Mutex::new(event_log)),
            manifest: session_manifest.challenge_manifest,
            public_key_hex: session_manifest.public_key_hex,
            evidence_capture: session_manifest.evidence_capture,
            storage,
        })
    }
}

pub(crate) fn observer_evidence_capture() -> EvidenceCapture {
    EvidenceCapture {
        terminal_io: "external_observer".into(),
        notes: vec![
            "The agent process was not launched by agent-ledger. This session records lifecycle, workspace file events, workspace hashes, git diffs, and any explicitly ingested/status-derived evidence.".into(),
            "Observer mode does not capture full terminal stdin/stdout unless a future collector or ingest command appends that evidence.".into(),
        ],
    }
}

pub(crate) fn create_session(
    agent: String,
    evidence_capture: EvidenceCapture,
) -> anyhow::Result<SessionContext> {
    create_session_at(env::current_dir()?, agent, evidence_capture)
}

pub(crate) fn capture_session_snapshot(
    context: &SessionContext,
    operation: &str,
) -> anyhow::Result<WorkspaceHash> {
    let workspace_hash = compute_workspace_hash(&context.workspace_dir)?;
    let timestamp = Utc::now().timestamp_millis();
    fs::write(
        context
            .session_dir
            .join("workspace.snapshots")
            .join(format!("{timestamp}.json")),
        workspace_hash.to_json()?,
    )?;

    let diff_capture = capture_git_diff(&context.workspace_dir);
    let diff_contents = diff_capture.file_contents();
    if let Some(contents) = &diff_contents {
        fs::write(
            context
                .session_dir
                .join("diffs")
                .join(format!("{timestamp}.diff")),
            contents,
        )?;
    }

    let mut event_log = context
        .event_log
        .lock()
        .map_err(|_| anyhow::anyhow!("event log mutex poisoned"))?;
    event_log.append(
        EventType::WorkspaceHashSnapshot,
        serde_json::to_value(&workspace_hash)?,
    )?;
    if let Some(payload) = diff_capture.warning_payload(operation) {
        event_log.append(EventType::Warning, payload)?;
    }
    if diff_contents.is_some() {
        event_log.append(EventType::GitDiffSnapshot, diff_capture.event_payload())?;
    }

    Ok(workspace_hash)
}

pub(crate) fn finish_session(
    mut context: SessionContext,
    status: SessionStatus,
    final_workspace_hash: String,
    extra_payload: serde_json::Value,
) -> anyhow::Result<()> {
    context.session.final_workspace_hash = Some(final_workspace_hash.clone());
    context.session.finished_at = Some(Utc::now());
    context.session.status = status;
    context.storage.save_session(&context.session)?;
    context
        .storage
        .update_session_status(&context.session.id, context.session.status.clone())?;

    let mut payload = json!({
        "status": context.session.status.to_string(),
        "final_workspace_hash": context.session.final_workspace_hash,
    });
    if let (Some(payload_object), Some(extra_object)) =
        (payload.as_object_mut(), extra_payload.as_object())
    {
        for (key, value) in extra_object {
            payload_object.insert(key.clone(), value.clone());
        }
    }

    {
        let mut event_log = context
            .event_log
            .lock()
            .map_err(|_| anyhow::anyhow!("event log mutex poisoned"))?;
        event_log.append(EventType::SessionFinished, payload)?;
    }

    write_session_manifest(
        &session_manifest_path(&context.session_dir),
        &SessionManifestFile {
            session: context.session,
            challenge_manifest: context.manifest,
            public_key_hex: context.public_key_hex,
            evidence_capture: context.evidence_capture,
        },
    )?;

    Ok(())
}

fn create_session_at(
    workspace_dir: PathBuf,
    agent: String,
    evidence_capture: EvidenceCapture,
) -> anyhow::Result<SessionContext> {
    let manifest = load_manifest_or_default_at(&workspace_dir)?;
    ensure_agent_allowed(&manifest, &agent)?;

    let session_id = SessionId::new();
    let session_dir = sessions_dir(&workspace_dir).join(&session_id.0);
    create_session_dirs(&session_dir)?;

    let storage = Storage::open(&session_db_path(&session_dir))?;
    let key = SessionKey::generate();
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
        ledger_dir: ledger_dir(&workspace_dir),
        manifest: manifest.clone(),
    });
    session.baseline_commit = baseline_commit.clone();
    session.baseline_workspace_hash = Some(baseline_workspace_hash.total_hash.clone());
    storage.save_session(&session)?;

    let public_key_hex = key.public_key_hex();
    write_session_manifest(
        &session_manifest_path(&session_dir),
        &SessionManifestFile {
            session: session.clone(),
            challenge_manifest: manifest.clone(),
            public_key_hex: public_key_hex.clone(),
            evidence_capture: evidence_capture.clone(),
        },
    )?;
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

    let context = SessionContext {
        session,
        session_dir,
        workspace_dir,
        event_log: Arc::new(Mutex::new(event_log)),
        manifest,
        public_key_hex,
        evidence_capture,
        storage,
    };
    capture_session_snapshot(&context, "start")?;

    Ok(context)
}

fn load_manifest_or_default_at(
    workspace_dir: &std::path::Path,
) -> anyhow::Result<ChallengeManifest> {
    let path = workspace_dir.join("ledger.yaml");
    if path.exists() {
        Ok(ChallengeManifest::load_from_file(&path)?)
    } else {
        let manifest = ChallengeManifest::default_manifest();
        manifest.validate()?;
        Ok(manifest)
    }
}

fn ensure_agent_allowed(manifest: &ChallengeManifest, agent: &str) -> anyhow::Result<()> {
    if manifest
        .allowed_agents
        .iter()
        .any(|allowed| allowed == agent)
    {
        Ok(())
    } else {
        anyhow::bail!("agent '{agent}' is not allowed by ledger.yaml")
    }
}

fn create_session_dirs(session_dir: &std::path::Path) -> anyhow::Result<()> {
    fs::create_dir_all(session_dir.join("workspace.snapshots"))?;
    fs::create_dir_all(session_dir.join("diffs"))?;
    fs::create_dir_all(session_dir.join("test-results"))?;
    fs::create_dir_all(session_dir.join("final"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use agent_ledger_core::{
        session::SessionStatus,
        status::{event_log_path, load_events, read_session_manifest, session_manifest_path},
    };
    use serde_json::json;

    use super::*;

    fn test_workspace(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let dir = std::env::current_dir()
            .expect("current dir")
            .join(".agent-ledger-test-artifacts")
            .join(format!("{name}-{stamp}"));
        fs::create_dir_all(&dir).expect("create test workspace");
        dir
    }

    #[test]
    fn create_session_writes_ledger_compatible_artifacts() {
        let workspace = test_workspace("create-session");
        fs::write(workspace.join("README.md"), "hello").expect("write workspace file");

        let context = create_session_at(
            workspace.clone(),
            "copilot".into(),
            observer_evidence_capture(),
        )
        .expect("create observer session");

        assert!(context.session_dir.join("session.db").exists());
        assert!(context.session_dir.join("session.key").exists());
        assert!(context.session_dir.join("events.jsonl").exists());
        assert!(context.session_dir.join("workspace.snapshots").exists());
        assert!(context.session_dir.join("diffs").exists());

        let manifest = read_session_manifest(&session_manifest_path(&context.session_dir))
            .expect("read session manifest");
        assert_eq!(manifest.session.status, SessionStatus::Active);
        assert_eq!(manifest.evidence_capture.terminal_io, "external_observer");

        let events = load_events(&event_log_path(&context.session_dir)).expect("load events");
        assert!(events
            .iter()
            .any(|event| event.event_type == EventType::SessionStarted));
        assert!(events
            .iter()
            .any(|event| event.event_type == EventType::WorkspaceHashSnapshot));

        fs::remove_dir_all(workspace).expect("cleanup workspace");
    }

    #[test]
    fn finish_session_updates_manifest_and_appends_finished_event() {
        let workspace = test_workspace("finish-session");
        fs::write(workspace.join("main.txt"), "before").expect("write workspace file");
        let context = create_session_at(
            workspace.clone(),
            "copilot".into(),
            observer_evidence_capture(),
        )
        .expect("create observer session");
        let session_dir = context.session_dir.clone();
        let workspace_hash = capture_session_snapshot(&context, "test").expect("capture snapshot");

        finish_session(
            context,
            SessionStatus::Finished,
            workspace_hash.total_hash,
            json!({ "reason": "test_complete" }),
        )
        .expect("finish session");

        let manifest =
            read_session_manifest(&session_manifest_path(&session_dir)).expect("read manifest");
        assert_eq!(manifest.session.status, SessionStatus::Finished);
        assert!(manifest.session.finished_at.is_some());
        assert!(manifest.session.final_workspace_hash.is_some());

        let events = load_events(&event_log_path(&session_dir)).expect("load events");
        let finished = events
            .iter()
            .find(|event| event.event_type == EventType::SessionFinished)
            .expect("session finished event");
        assert_eq!(finished.payload["reason"], "test_complete");

        fs::remove_dir_all(workspace).expect("cleanup workspace");
    }
}
