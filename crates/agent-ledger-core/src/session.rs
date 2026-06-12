use std::{fmt, path::PathBuf, str::FromStr};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::manifest::ChallengeManifest;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new() -> Self {
        Self(format!("session-{}", Uuid::new_v4()))
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for SessionId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for SessionId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub session_id: SessionId,
    pub agent_name: String,
    pub workspace_dir: PathBuf,
    pub ledger_dir: PathBuf,
    pub manifest: ChallengeManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Finished,
    Failed,
}

impl fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionStatus::Active => f.write_str("active"),
            SessionStatus::Finished => f.write_str("finished"),
            SessionStatus::Failed => f.write_str("failed"),
        }
    }
}

impl FromStr for SessionStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(SessionStatus::Active),
            "finished" => Ok(SessionStatus::Finished),
            "failed" => Ok(SessionStatus::Failed),
            _ => Err(format!("unknown session status: {s}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub agent_name: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub baseline_commit: Option<String>,
    pub baseline_workspace_hash: Option<String>,
    pub final_workspace_hash: Option<String>,
    pub status: SessionStatus,
}

impl Session {
    pub fn new(config: &SessionConfig) -> Self {
        let _ = (&config.workspace_dir, &config.ledger_dir, &config.manifest);
        Self {
            id: config.session_id.clone(),
            agent_name: config.agent_name.clone(),
            started_at: Utc::now(),
            finished_at: None,
            baseline_commit: None,
            baseline_workspace_hash: None,
            final_workspace_hash: None,
            status: SessionStatus::Active,
        }
    }

    pub fn finish(&mut self) {
        self.finished_at = Some(Utc::now());
        self.status = SessionStatus::Finished;
    }
}
