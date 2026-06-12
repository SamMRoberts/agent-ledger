pub mod doctor;
pub mod init;
pub mod start;
pub mod snapshot;
pub mod status;
pub mod submit;
pub mod verify;

use std::{fs, path::{Path, PathBuf}};

use agent_ledger_agents::CommandSpec;
use agent_ledger_core::{
    event::{Event, EventLog, EventType},
    manifest::ChallengeManifest,
    session::{Session, SessionStatus},
};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

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
    #[serde(default)]
    pub evidence_capture: EvidenceCapture,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct EvidenceCapture {
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

impl EvidenceCapture {
    pub(crate) fn from_command_spec(spec: &CommandSpec) -> Self {
        if spec.interactive {
            Self {
                terminal_io: "interactive_pty_transcript".into(),
                notes: vec![
                    "The agent inherited terminal interaction through a PTY transcript capture path. agent-ledger records lifecycle and snapshots, and stores replayed stdout transcript lines when capture is available.".into(),
                ],
            }
        } else {
            Self {
                terminal_io: "captured_stdout_stderr".into(),
                notes: vec![
                    "The agent process stdout and stderr were captured line-by-line by agent-ledger. stdin was not recorded.".into(),
                ],
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitDiffCapture {
    diff: Option<String>,
    error: Option<String>,
}

impl GitDiffCapture {
    pub(crate) fn captured(&self) -> bool {
        self.error.is_none()
    }

    pub(crate) fn event_payload(&self) -> serde_json::Value {
        match (&self.diff, &self.error) {
            (Some(diff), None) => json!({
                "captured": true,
                "diff": diff,
            }),
            (_, Some(error)) => json!({
                "captured": false,
                "error": error,
            }),
            (None, None) => json!({
                "captured": false,
                "reason": "not_a_git_repository",
            }),
        }
    }

    pub(crate) fn warning_payload(&self, operation: &str) -> Option<serde_json::Value> {
        self.error.as_ref().map(|error| {
            json!({
                "kind": "git_diff_capture_failed",
                "operation": operation,
                "error": error,
            })
        })
    }

    pub(crate) fn file_contents(&self) -> Option<String> {
        match (&self.diff, &self.error) {
            (Some(diff), None) => Some(diff.clone()),
            (_, Some(error)) => Some(format!(
                "# agent-ledger degraded evidence\nreason: git diff capture failed\nerror: {error}\n"
            )),
            (None, None) => Some(
                "# agent-ledger note\nreason: workspace is not a git repository\n".into(),
            ),
        }
    }
}

pub(crate) fn capture_git_diff(repo_dir: &Path) -> GitDiffCapture {
    if !agent_ledger_runner::git::is_git_repo(repo_dir) {
        return GitDiffCapture {
            diff: None,
            error: None,
        };
    }

    match agent_ledger_runner::git::get_diff(repo_dir) {
        Ok(diff) => GitDiffCapture {
            diff: Some(diff),
            error: None,
        },
        Err(error) => GitDiffCapture {
            diff: None,
            error: Some(error.to_string()),
        },
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_command_spec_marks_reduced_terminal_capture() {
        let spec = CommandSpec {
            program: "copilot".into(),
            args: Vec::new(),
            env: Default::default(),
            interactive: true,
        };

        let capture = EvidenceCapture::from_command_spec(&spec);

        assert_eq!(capture.terminal_io, "interactive_pty_transcript");
        assert!(!capture.notes.is_empty());
    }

    #[test]
    fn degraded_git_diff_file_contents_are_explicit() {
        let capture = GitDiffCapture {
            diff: None,
            error: Some("simulated failure".into()),
        };

        let file_contents = capture.file_contents().expect("degraded evidence marker");

        assert!(file_contents.contains("degraded evidence"));
        assert!(file_contents.contains("simulated failure"));
        assert_eq!(capture.event_payload()["captured"], false);
    }

    #[test]
    fn non_git_workspace_file_contents_are_explicit() {
        let capture = GitDiffCapture {
            diff: None,
            error: None,
        };

        let file_contents = capture.file_contents().expect("non-git note");

        assert!(file_contents.contains("not a git repository"));
        assert_eq!(capture.event_payload()["reason"], "not_a_git_repository");
    }
}
