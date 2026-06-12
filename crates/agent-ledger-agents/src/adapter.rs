use std::{collections::HashMap, env, path::{Path, PathBuf}};

#[derive(Debug, Clone)]
pub struct AgentDetection {
    pub found: bool,
    pub version: Option<String>,
    pub path: Option<PathBuf>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    /// When true, stdin/stdout/stderr are inherited from the parent process
    /// so the user can interact with the agent directly in their terminal.
    pub interactive: bool,
}

#[derive(Debug, Clone)]
pub struct AgentParsedEvent {
    pub event_type: String,
    pub data: serde_json::Value,
}

pub trait AgentAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn detect(&self) -> anyhow::Result<AgentDetection>;
    fn launch_command(&self, workspace_dir: &Path) -> anyhow::Result<CommandSpec>;
    fn token_status_command(&self) -> Option<CommandSpec>;
    fn parse_output_event(&self, line: &str) -> Vec<AgentParsedEvent>;
}

pub(crate) fn binary_in_path(name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

pub(crate) fn version_string(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stdout.is_empty() {
        Some(stdout)
    } else if !stderr.is_empty() {
        Some(stderr)
    } else {
        None
    }
}
