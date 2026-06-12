use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use anyhow::Context;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    hash_chain::{compute_event_hash, compute_payload_hash},
    session::SessionId,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    SessionStarted,
    SessionFinished,
    AgentStarted,
    AgentStopped,
    AgentStdin,
    AgentStdout,
    AgentStderr,
    FileCreated,
    FileModified,
    FileDeleted,
    FileRenamed,
    GitDiffSnapshot,
    WorkspaceHashSnapshot,
    TokenReport,
    TestRunStarted,
    TestRunFinished,
    SubmissionCreated,
    VerificationResult,
    Warning,
    Error,
}

impl EventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EventType::SessionStarted => "session_started",
            EventType::SessionFinished => "session_finished",
            EventType::AgentStarted => "agent_started",
            EventType::AgentStopped => "agent_stopped",
            EventType::AgentStdin => "agent_stdin",
            EventType::AgentStdout => "agent_stdout",
            EventType::AgentStderr => "agent_stderr",
            EventType::FileCreated => "file_created",
            EventType::FileModified => "file_modified",
            EventType::FileDeleted => "file_deleted",
            EventType::FileRenamed => "file_renamed",
            EventType::GitDiffSnapshot => "git_diff_snapshot",
            EventType::WorkspaceHashSnapshot => "workspace_hash_snapshot",
            EventType::TokenReport => "token_report",
            EventType::TestRunStarted => "test_run_started",
            EventType::TestRunFinished => "test_run_finished",
            EventType::SubmissionCreated => "submission_created",
            EventType::VerificationResult => "verification_result",
            EventType::Warning => "warning",
            EventType::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    pub timestamp: String,
    pub session_id: String,
    pub event_type: EventType,
    pub payload: Value,
    pub payload_hash: String,
    pub prev_hash: String,
    pub event_hash: String,
}

pub struct EventLog {
    path: PathBuf,
    session_id: SessionId,
    next_seq: u64,
    prev_hash: String,
}

impl EventLog {
    pub fn new(path: &Path, session_id: SessionId) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if !path.exists() {
            fs::File::create(path)?;
        }
        let events = Self::load_all(path)?;
        let (next_seq, prev_hash) = events
            .last()
            .map(|event| (event.seq + 1, event.event_hash.clone()))
            .unwrap_or((0, "genesis".to_string()));
        Ok(Self {
            path: path.to_path_buf(),
            session_id,
            next_seq,
            prev_hash,
        })
    }

    pub fn append(&mut self, event_type: EventType, payload: Value) -> anyhow::Result<Event> {
        let timestamp = Utc::now().to_rfc3339();
        let payload_hash = compute_payload_hash(&payload);
        let event_hash = compute_event_hash(
            self.next_seq,
            &timestamp,
            &self.session_id.0,
            &event_type,
            &payload_hash,
            &self.prev_hash,
        );
        let event = Event {
            seq: self.next_seq,
            timestamp,
            session_id: self.session_id.0.clone(),
            event_type,
            payload,
            payload_hash,
            prev_hash: self.prev_hash.clone(),
            event_hash: event_hash.clone(),
        };

        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.path)
            .with_context(|| format!("opening event log at {}", self.path.display()))?;
        serde_json::to_writer(&mut file, &event)?;
        file.write_all(b"\n")?;

        self.next_seq += 1;
        self.prev_hash = event_hash;
        Ok(event)
    }

    pub fn load_all(path: &Path) -> anyhow::Result<Vec<Event>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(path)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            events.push(serde_json::from_str(&line)?);
        }
        Ok(events)
    }
}
