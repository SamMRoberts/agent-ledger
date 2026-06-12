use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use agent_ledger_agents::CommandSpec;
use agent_ledger_core::event::{EventLog, EventType};
use anyhow::{anyhow, Context};
use serde_json::json;
use tokio::io::{AsyncBufReadExt, BufReader};

pub type AgentLineHandler = Arc<dyn Fn(EventType, &str) -> anyhow::Result<()> + Send + Sync>;

pub struct ProcessRunner {
    session_id: String,
    event_log: Arc<Mutex<EventLog>>,
    line_handler: Option<AgentLineHandler>,
}

impl ProcessRunner {
    pub fn new(session_id: String, event_log: Arc<Mutex<EventLog>>) -> Self {
        Self {
            session_id,
            event_log,
            line_handler: None,
        }
    }

    pub fn with_line_handler(mut self, line_handler: AgentLineHandler) -> Self {
        self.line_handler = Some(line_handler);
        self
    }

    fn append_line_event(
        event_log: &Arc<Mutex<EventLog>>,
        line_handler: &Option<AgentLineHandler>,
        event_type: EventType,
        line: &str,
    ) -> anyhow::Result<()> {
        {
            let mut log = event_log
                .lock()
                .map_err(|_| anyhow!("event log mutex poisoned"))?;
            log.append(event_type.clone(), json!({ "line": line }))?;
        }

        if let Some(handler) = line_handler {
            handler(event_type, line)?;
        }

        Ok(())
    }

    pub async fn run_agent(&self, spec: &CommandSpec, workspace_dir: &Path) -> anyhow::Result<i32> {
        {
            let mut log = self
                .event_log
                .lock()
                .map_err(|_| anyhow!("event log mutex poisoned"))?;
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
            let line_handler = self.line_handler.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Some(line) = lines.next_line().await? {
                    Self::append_line_event(
                        &event_log,
                        &line_handler,
                        EventType::AgentStdout,
                        &line,
                    )?;
                }
                Ok::<(), anyhow::Error>(())
            })
        });

        let stderr_task = child.stderr.take().map(|stderr| {
            let event_log = Arc::clone(&self.event_log);
            let line_handler = self.line_handler.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Some(line) = lines.next_line().await? {
                    Self::append_line_event(
                        &event_log,
                        &line_handler,
                        EventType::AgentStderr,
                        &line,
                    )?;
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
            let mut log = self
                .event_log
                .lock()
                .map_err(|_| anyhow!("event log mutex poisoned"))?;
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
    /// through `script` to keep PTY semantics while also tailing a transcript
    /// into AgentStdout events while the process is still running.
    async fn run_interactive(
        &self,
        spec: &CommandSpec,
        workspace_dir: &Path,
    ) -> anyhow::Result<i32> {
        let transcript_path =
            std::env::temp_dir().join(format!("agent-ledger-{}-transcript.log", self.session_id));

        let mut command = tokio::process::Command::new("script");
        command
            .arg("-q")
            .arg("-F")
            .arg(&transcript_path)
            .arg(&spec.program)
            .args(&spec.args)
            .current_dir(workspace_dir)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .envs(&spec.env);

        let mut child = command.spawn().with_context(|| {
            format!(
                "spawning interactive agent process {} via script",
                spec.program
            )
        })?;

        let tail_done = Arc::new(AtomicBool::new(false));
        let tail_task = {
            let transcript_path = transcript_path.clone();
            let event_log = Arc::clone(&self.event_log);
            let line_handler = self.line_handler.clone();
            let tail_done = Arc::clone(&tail_done);
            tokio::spawn(async move {
                Self::tail_transcript_until_done(
                    transcript_path,
                    event_log,
                    line_handler,
                    tail_done,
                )
                .await
            })
        };

        let status = child.wait().await?;
        tail_done.store(true, Ordering::SeqCst);
        tail_task.await??;
        let _ = fs::remove_file(&transcript_path);

        {
            let mut log = self
                .event_log
                .lock()
                .map_err(|_| anyhow!("event log mutex poisoned"))?;
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

    async fn tail_transcript_until_done(
        transcript_path: PathBuf,
        event_log: Arc<Mutex<EventLog>>,
        line_handler: Option<AgentLineHandler>,
        done: Arc<AtomicBool>,
    ) -> anyhow::Result<()> {
        let mut processed_bytes = 0;
        let mut pending = String::new();

        loop {
            if let Ok(transcript) = fs::read_to_string(&transcript_path) {
                if transcript.len() > processed_bytes {
                    pending.push_str(&transcript[processed_bytes..]);
                    processed_bytes = transcript.len();

                    let complete = pending.ends_with('\n') || done.load(Ordering::SeqCst);
                    if complete {
                        for line in pending.lines() {
                            Self::append_transcript_line(&event_log, &line_handler, line)?;
                        }
                        pending.clear();
                    } else if let Some(last_newline) = pending.rfind('\n') {
                        let complete_lines = pending[..last_newline].to_string();
                        let remainder = pending[last_newline + 1..].to_string();
                        for line in complete_lines.lines() {
                            Self::append_transcript_line(&event_log, &line_handler, line)?;
                        }
                        pending = remainder;
                    }
                }
            }

            if done.load(Ordering::SeqCst) {
                break;
            }

            tokio::time::sleep(Duration::from_millis(250)).await;
        }

        if !pending.trim().is_empty() {
            Self::append_transcript_line(&event_log, &line_handler, &pending)?;
        }

        Ok(())
    }

    fn append_transcript_line(
        event_log: &Arc<Mutex<EventLog>>,
        line_handler: &Option<AgentLineHandler>,
        line: &str,
    ) -> anyhow::Result<()> {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("Script started")
            || trimmed.starts_with("Script done")
        {
            return Ok(());
        }

        Self::append_line_event(event_log, line_handler, EventType::AgentStdout, trimmed)
    }
}
