pub mod doctor;
pub mod init;
pub mod start;
pub mod snapshot;
pub mod status;
pub mod submit;
pub mod verify;

use std::{fs, path::{Path, PathBuf}};

use agent_ledger_core::{
    event::{Event, EventLog, EventType},
    manifest::ChallengeManifest,
    session::{Session, SessionStatus},
};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

pub(crate) fn ledger_dir() -> PathBuf {
    PathBuf::from(".ledger")
}

pub(crate) fn sessions_dir() -> PathBuf {
    ledger_dir().join("sessions")
}

pub(crate) fn session_db_path(session_dir: &Path) -> PathBuf {
    session_dir.join("session.db")
}

pub(crate) fn event_log_path(session_dir: &Path) -> PathBuf {
    session_dir.join("events.jsonl")
}

pub(crate) fn session_manifest_path(session_dir: &Path) -> PathBuf {
    session_dir.join("session_manifest.json")
}

pub(crate) fn session_key_path(session_dir: &Path) -> PathBuf {
    session_dir.join("session.key")
}

pub(crate) fn load_manifest_or_default() -> Result<ChallengeManifest> {
    let path = PathBuf::from("ledger.yaml");
    if path.exists() {
        Ok(ChallengeManifest::load_from_file(&path)?)
    } else {
        let manifest = ChallengeManifest::default_manifest();
        manifest.validate()?;
        Ok(manifest)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SessionManifestFile {
    pub session: Session,
    pub challenge_manifest: ChallengeManifest,
    pub public_key_hex: String,
}

pub(crate) fn write_session_manifest(path: &Path, manifest: &SessionManifestFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(manifest)?)?;
    Ok(())
}

pub(crate) fn read_session_manifest(path: &Path) -> Result<SessionManifestFile> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

pub(crate) fn list_session_dirs() -> Result<Vec<PathBuf>> {
    let base = sessions_dir();
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

pub(crate) fn active_or_latest_session_dir() -> Result<Option<PathBuf>> {
    let dirs = list_session_dirs()?;
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

pub(crate) fn latest_workspace_hash(events: &[Event]) -> Option<String> {
    events.iter().rev().find_map(|event| {
        if event.event_type == EventType::WorkspaceHashSnapshot {
            event.payload.get("total_hash")?.as_str().map(ToOwned::to_owned)
        } else {
            None
        }
    })
}

pub(crate) fn load_events(path: &Path) -> Result<Vec<Event>> {
    EventLog::load_all(path)
}

pub(crate) fn required_file<'a>(files: &'a std::collections::HashMap<String, Vec<u8>>, name: &str) -> Result<&'a [u8]> {
    files
        .get(name)
        .map(Vec::as_slice)
        .ok_or_else(|| anyhow!("missing required bundle file: {name}"))
}
