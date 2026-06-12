use std::{collections::HashMap, path::Path};

use serde_json::json;

use crate::adapter::{binary_in_path, version_string, AgentAdapter, AgentDetection, AgentParsedEvent, CommandSpec};

pub struct CopilotAdapter;

impl AgentAdapter for CopilotAdapter {
    fn name(&self) -> &'static str {
        "copilot"
    }

    fn detect(&self) -> anyhow::Result<AgentDetection> {
        let path = binary_in_path("copilot");
        let version = version_string("copilot", &["--version"])
            .or_else(|| version_string("copilot", &["version"]));
        let notes = vec!["Copilot CLI detection is based on the copilot binary being available in PATH".into()];
        Ok(AgentDetection {
            found: path.is_some(),
            version,
            path,
            notes,
        })
    }

    fn launch_command(&self, _workspace_dir: &Path) -> anyhow::Result<CommandSpec> {
        Ok(CommandSpec {
            program: "copilot".into(),
            args: Vec::new(),
            env: HashMap::new(),
            interactive: true,
        })
    }

    fn token_status_command(&self) -> Option<CommandSpec> {
        None
    }

    fn parse_output_event(&self, line: &str) -> Vec<AgentParsedEvent> {
        let event_type = if line.to_ascii_lowercase().contains("error") {
            "error"
        } else {
            "stdout"
        };
        vec![AgentParsedEvent {
            event_type: event_type.into(),
            data: json!({ "line": line }),
        }]
    }
}
