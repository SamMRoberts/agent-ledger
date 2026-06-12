use std::{env, fs, fs::File};

use agent_ledger_core::{
    event::{EventLog, EventType},
    session::SessionStatus,
    signing::SessionKey,
    storage::Storage,
    workspace::compute_workspace_hash,
};
use chrono::Utc;
use flate2::{write::GzEncoder, Compression};
use serde_json::json;
use tar::Builder;

use super::{
    active_or_latest_session_dir, capture_git_diff, event_log_path, load_events,
    read_session_manifest, session_db_path, session_key_path, session_manifest_path,
    write_session_manifest,
};

pub async fn run() -> anyhow::Result<()> {
    let session_dir =
        active_or_latest_session_dir()?.ok_or_else(|| anyhow::anyhow!("no sessions found"))?;
    let manifest_path = session_manifest_path(&session_dir);
    let mut session_manifest = read_session_manifest(&manifest_path)?;
    let workspace_dir = env::current_dir()?;
    let final_dir = session_dir.join("final");
    fs::create_dir_all(&final_dir)?;

    let workspace_hash = compute_workspace_hash(&workspace_dir)?;
    session_manifest.session.final_workspace_hash = Some(workspace_hash.total_hash.clone());
    if session_manifest.session.finished_at.is_none() {
        session_manifest.session.finished_at = Some(Utc::now());
    }
    if session_manifest.session.status == SessionStatus::Active {
        session_manifest.session.status = SessionStatus::Finished;
    }

    let workspace_hash_path = final_dir.join("workspace_hash.json");
    fs::write(&workspace_hash_path, workspace_hash.to_json()?)?;
    let diff_path = final_dir.join("final.diff");
    let diff_capture = capture_git_diff(&workspace_dir);
    if let Some(diff_contents) = diff_capture.file_contents() {
        fs::write(&diff_path, diff_contents)?;
    }

    let mut event_log = EventLog::new(
        &event_log_path(&session_dir),
        session_manifest.session.id.clone(),
    )?;
    event_log.append(
        EventType::SubmissionCreated,
        json!({
            "bundle_path": "final/submission.tar.gz",
            "final_workspace_hash": session_manifest.session.final_workspace_hash,
            "git_diff_captured": diff_capture.captured(),
        }),
    )?;
    if let Some(payload) = diff_capture.warning_payload("submit") {
        event_log.append(EventType::Warning, payload)?;
    }
    if diff_capture.file_contents().is_some() {
        event_log.append(EventType::GitDiffSnapshot, diff_capture.event_payload())?;
    }

    let events = load_events(&event_log_path(&session_dir))?;
    let events_hash = events
        .last()
        .map(|event| event.event_hash.clone())
        .unwrap_or_else(|| "genesis".into());
    let signing_input = format!(
        "{}{}{}",
        session_manifest.session.id,
        session_manifest
            .session
            .final_workspace_hash
            .clone()
            .unwrap_or_default(),
        events_hash
    );
    let digest = blake3::hash(signing_input.as_bytes());
    let key = SessionKey::load_from_file(&session_key_path(&session_dir))?;
    let signature_hex = hex::encode(key.sign(digest.as_bytes()).to_bytes());
    let signature_path = final_dir.join("signature.ed25519");
    fs::write(&signature_path, signature_hex)?;

    write_session_manifest(&manifest_path, &session_manifest)?;
    Storage::open(&session_db_path(&session_dir))?.save_session(&session_manifest.session)?;

    let bundle_path = final_dir.join("submission.tar.gz");
    let bundle_file = File::create(&bundle_path)?;
    let encoder = GzEncoder::new(bundle_file, Compression::default());
    let mut archive = Builder::new(encoder);
    archive.append_path_with_name(&manifest_path, "session_manifest.json")?;
    archive.append_path_with_name(event_log_path(&session_dir), "events.jsonl")?;
    archive.append_path_with_name(&workspace_hash_path, "workspace_hash.json")?;
    archive.append_path_with_name(&diff_path, "final.diff")?;
    archive.append_path_with_name(&signature_path, "signature.ed25519")?;
    archive.finish()?;

    println!("Created submission bundle at {}", bundle_path.display());
    Ok(())
}
