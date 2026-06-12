use std::{collections::HashMap, path::Path};

use serde_json::json;

use crate::adapter::{binary_in_path, version_string, AgentAdapter, AgentDetection, AgentParsedEvent, CommandSpec};

pub struct CodexAdapter;

impl AgentAdapter for CodexAdapter {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn detect(&self) -> anyhow::Result<AgentDetection> {
        let path = binary_in_path("codex");
        let version = version_string("codex", &["--version"]).or_else(|| version_string("codex", &["version"]));
        Ok(AgentDetection {
            found: path.is_some(),
            version,
            path,
            notes: vec!["Codex detection is based on the codex binary being available in PATH".into()],
        })
    }

    fn launch_command(&self, _workspace_dir: &Path) -> anyhow::Result<CommandSpec> {
        Ok(CommandSpec {
            program: "codex".into(),
            args: Vec::new(),
            env: HashMap::new(),
            interactive: false,
        })
    }

    fn token_status_command(&self) -> Option<CommandSpec> {
        None
    }

    fn parse_output_event(&self, line: &str) -> Vec<AgentParsedEvent> {
        vec![AgentParsedEvent {
            event_type: "stdout".into(),
            data: json!({ "line": line }),
        }]
    }
}
