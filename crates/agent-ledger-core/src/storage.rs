use std::path::Path;

use anyhow::Context;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use crate::session::{Session, SessionId, SessionStatus};

pub struct Storage {
    conn: Connection,
}

impl Storage {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening sqlite database at {}", path.display()))?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                agent_name TEXT NOT NULL,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                baseline_commit TEXT,
                baseline_workspace_hash TEXT,
                final_workspace_hash TEXT,
                status TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            ",
        )?;
        Ok(Self { conn })
    }

    pub fn save_session(&self, session: &Session) -> anyhow::Result<()> {
        self.conn.execute(
            "
            INSERT INTO sessions (
                id,
                agent_name,
                started_at,
                finished_at,
                baseline_commit,
                baseline_workspace_hash,
                final_workspace_hash,
                status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(id) DO UPDATE SET
                agent_name = excluded.agent_name,
                started_at = excluded.started_at,
                finished_at = excluded.finished_at,
                baseline_commit = excluded.baseline_commit,
                baseline_workspace_hash = excluded.baseline_workspace_hash,
                final_workspace_hash = excluded.final_workspace_hash,
                status = excluded.status
            ",
            params![
                session.id.0,
                session.agent_name,
                session.started_at.to_rfc3339(),
                session.finished_at.map(|dt| dt.to_rfc3339()),
                session.baseline_commit,
                session.baseline_workspace_hash,
                session.final_workspace_hash,
                session.status.to_string(),
            ],
        )?;
        Ok(())
    }

    pub fn load_session(&self, id: &SessionId) -> anyhow::Result<Option<Session>> {
        let session = self
            .conn
            .query_row(
                "
                SELECT id, agent_name, started_at, finished_at, baseline_commit,
                       baseline_workspace_hash, final_workspace_hash, status
                FROM sessions WHERE id = ?1
                ",
                params![id.0],
                |row| {
                    let started_at: String = row.get(2)?;
                    let finished_at: Option<String> = row.get(3)?;
                    let status: String = row.get(7)?;
                    Ok(Session {
                        id: SessionId(row.get(0)?),
                        agent_name: row.get(1)?,
                        started_at: parse_rfc3339(&started_at).map_err(to_sql_error)?,
                        finished_at: finished_at
                            .as_deref()
                            .map(parse_rfc3339)
                            .transpose()
                            .map_err(to_sql_error)?,
                        baseline_commit: row.get(4)?,
                        baseline_workspace_hash: row.get(5)?,
                        final_workspace_hash: row.get(6)?,
                        status: status.parse().map_err(to_sql_error)?,
                    })
                },
            )
            .optional()?;
        Ok(session)
    }

    pub fn update_session_status(
        &self,
        id: &SessionId,
        status: SessionStatus,
    ) -> anyhow::Result<()> {
        let finished_at = match status {
            SessionStatus::Active => None,
            SessionStatus::Finished | SessionStatus::Failed => Some(Utc::now().to_rfc3339()),
        };
        self.conn.execute(
            "UPDATE sessions SET status = ?2, finished_at = COALESCE(?3, finished_at) WHERE id = ?1",
            params![id.0, status.to_string(), finished_at],
        )?;
        Ok(())
    }
}

fn parse_rfc3339(value: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    Ok(DateTime::parse_from_rfc3339(value)?.with_timezone(&Utc))
}

fn to_sql_error<E>(err: E) -> rusqlite::Error
where
    E: std::fmt::Display,
{
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            err.to_string(),
        )),
    )
}
