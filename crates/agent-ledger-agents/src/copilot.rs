use std::{collections::HashMap, path::Path};

use serde_json::json;

use crate::adapter::{binary_in_path, version_string, AgentAdapter, AgentDetection, AgentParsedEvent, CommandSpec};

pub struct CopilotAdapter;

impl AgentAdapter for CopilotAdapter {
    fn name(&self) -> &'static str {
        "copilot"
    }

    fn detect(&self) -> anyhow::Result<AgentDetection> {
        let path = binary_in_path("gh");
        let version = version_string("gh", &["--version"]);
        let mut notes = Vec::new();
        if let Some(extension_output) = version_string("gh", &["extension", "list"]) {
            if extension_output.contains("copilot") {
                notes.push("GitHub Copilot extension detected".into());
            } else {
                notes.push("GitHub Copilot extension not listed by gh extension list".into());
            }
        }
        Ok(AgentDetection {
            found: path.is_some() && version.is_some(),
            version,
            path,
            notes,
        })
    }

    fn launch_command(&self, _workspace_dir: &Path) -> anyhow::Result<CommandSpec> {
        Ok(CommandSpec {
            program: "gh".into(),
            args: vec![
                "copilot".into(),
                "suggest".into(),
                "-t".into(),
                "shell".into(),
                "agent-ledger session started".into(),
            ],
            env: HashMap::new(),
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
