pub mod doctor;
pub mod init;
pub mod snapshot;
pub mod start;
pub mod status;
pub mod submit;
pub mod verify;

use std::{path::Path, path::PathBuf};

use agent_ledger_agents::CommandSpec;
use agent_ledger_core::{
    event::Event,
    manifest::ChallengeManifest,
    status::{
        active_or_latest_session_dir as core_active_or_latest_session_dir,
        event_log_path as core_event_log_path, ledger_dir as core_ledger_dir,
        load_events as core_load_events, read_session_manifest as core_read_session_manifest,
        session_db_path as core_session_db_path, session_key_path as core_session_key_path,
        session_manifest_path as core_session_manifest_path, sessions_dir as core_sessions_dir,
        write_session_manifest as core_write_session_manifest, EvidenceCapture,
        SessionManifestFile,
    },
};
use anyhow::Result;
use serde_json::json;

pub(crate) fn ledger_dir() -> PathBuf {
    core_ledger_dir(Path::new("."))
}

pub(crate) fn sessions_dir() -> PathBuf {
    core_sessions_dir(Path::new("."))
}

pub(crate) fn session_db_path(session_dir: &Path) -> PathBuf {
    core_session_db_path(session_dir)
}

pub(crate) fn event_log_path(session_dir: &Path) -> PathBuf {
    core_event_log_path(session_dir)
}

pub(crate) fn session_manifest_path(session_dir: &Path) -> PathBuf {
    core_session_manifest_path(session_dir)
}

pub(crate) fn session_key_path(session_dir: &Path) -> PathBuf {
    core_session_key_path(session_dir)
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

pub(crate) fn evidence_capture_from_command_spec(spec: &CommandSpec) -> EvidenceCapture {
    if spec.interactive {
        EvidenceCapture {
            terminal_io: "interactive_pty_transcript".into(),
            notes: vec![
                "The agent inherited terminal interaction through a PTY transcript capture path. agent-ledger records lifecycle and snapshots, tails transcript stdout when capture is available, and derives live usage reports from visible agent output.".into(),
            ],
        }
    } else {
        EvidenceCapture {
            terminal_io: "captured_stdout_stderr".into(),
            notes: vec![
                "The agent process stdout and stderr were captured line-by-line by agent-ledger. stdin was not recorded.".into(),
            ],
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

pub(crate) fn read_session_manifest(path: &Path) -> Result<SessionManifestFile> {
    core_read_session_manifest(path)
}

pub(crate) fn write_session_manifest(path: &Path, manifest: &SessionManifestFile) -> Result<()> {
    core_write_session_manifest(path, manifest)
}

pub(crate) fn active_or_latest_session_dir() -> Result<Option<PathBuf>> {
    core_active_or_latest_session_dir(Path::new("."))
}

pub(crate) fn load_events(path: &Path) -> Result<Vec<Event>> {
    core_load_events(path)
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

        let capture = evidence_capture_from_command_spec(&spec);

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
