use std::{fs, path::Path, process::Stdio, sync::{Arc, Mutex}};

use agent_ledger_agents::CommandSpec;
use agent_ledger_core::event::{EventLog, EventType};
use anyhow::{anyhow, Context};
use serde_json::json;
use tokio::io::{AsyncBufReadExt, BufReader};

pub struct ProcessRunner {
    session_id: String,
    event_log: Arc<Mutex<EventLog>>,
}

impl ProcessRunner {
    pub fn new(session_id: String, event_log: Arc<Mutex<EventLog>>) -> Self {
        Self { session_id, event_log }
    }

    pub async fn run_agent(&self, spec: &CommandSpec, workspace_dir: &Path) -> anyhow::Result<i32> {
        {
            let mut log = self.event_log.lock().map_err(|_| anyhow!("event log mutex poisoned"))?;
            log.append(
                EventType::AgentStarted,
                json!({
                    "session_id": self.session_id,
                    "program": spec.program,
                    "args": spec.args,
                    "interactive": spec.interactive,
                }),
            )?;
        }

        if spec.interactive {
            return self.run_interactive(spec, workspace_dir).await;
        }

        let mut command = tokio::process::Command::new(&spec.program);
        command
            .args(&spec.args)
            .current_dir(workspace_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .envs(&spec.env);

        let mut child = command
            .spawn()
            .with_context(|| format!("spawning agent process {}", spec.program))?;

        let stdout_task = child.stdout.take().map(|stdout| {
            let event_log = Arc::clone(&self.event_log);
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Some(line) = lines.next_line().await? {
                    let mut log = event_log.lock().map_err(|_| anyhow!("event log mutex poisoned"))?;
                    log.append(EventType::AgentStdout, json!({ "line": line }))?;
                }
                Ok::<(), anyhow::Error>(())
            })
        });

        let stderr_task = child.stderr.take().map(|stderr| {
            let event_log = Arc::clone(&self.event_log);
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Some(line) = lines.next_line().await? {
                    let mut log = event_log.lock().map_err(|_| anyhow!("event log mutex poisoned"))?;
                    log.append(EventType::AgentStderr, json!({ "line": line }))?;
                }
                Ok::<(), anyhow::Error>(())
            })
        });

        let status = child.wait().await?;

        if let Some(task) = stdout_task {
            task.await??;
        }
        if let Some(task) = stderr_task {
            task.await??;
        }

        {
            let mut log = self.event_log.lock().map_err(|_| anyhow!("event log mutex poisoned"))?;
            log.append(
                EventType::AgentStopped,
                json!({
                    "exit_code": status.code(),
                    "success": status.success(),
                }),
            )?;
        }

        Ok(status.code().unwrap_or_default())
    }

    /// Run the agent with stdio inherited from the parent process so the user
    /// can interact with it directly in their terminal. We invoke the process
    /// through `script` to keep PTY semantics while also capturing a transcript
    /// that is replayed into AgentStdout events after the process exits.
    async fn run_interactive(&self, spec: &CommandSpec, workspace_dir: &Path) -> anyhow::Result<i32> {
        let transcript_path = std::env::temp_dir().join(format!("agent-ledger-{}-transcript.log", self.session_id));

        let mut command = tokio::process::Command::new("script");
        command
            .arg("-q")
            .arg(&transcript_path)
            .arg(&spec.program)
            .args(&spec.args)
            .current_dir(workspace_dir)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .envs(&spec.env);

        let mut child = command
            .spawn()
            .with_context(|| format!("spawning interactive agent process {} via script", spec.program))?;

        let status = child.wait().await?;

        if let Ok(transcript) = fs::read_to_string(&transcript_path) {
            let mut log = self.event_log.lock().map_err(|_| anyhow!("event log mutex poisoned"))?;
            for line in transcript.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty()
                    || trimmed.starts_with("Script started")
                    || trimmed.starts_with("Script done")
                {
                    continue;
                }
                log.append(EventType::AgentStdout, json!({ "line": trimmed }))?;
            }
        }
        let _ = fs::remove_file(&transcript_path);

        {
            let mut log = self.event_log.lock().map_err(|_| anyhow!("event log mutex poisoned"))?;
            log.append(
                EventType::AgentStopped,
                json!({
                    "exit_code": status.code(),
                    "success": status.success(),
                }),
            )?;
        }

        Ok(status.code().unwrap_or_default())
    }
}
