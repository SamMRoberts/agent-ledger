use std::{collections::HashMap, path::Path};

use serde_json::json;

use crate::adapter::{binary_in_path, version_string, AgentAdapter, AgentDetection, AgentParsedEvent, CommandSpec};

pub struct CopilotAdapter;

fn parse_count_after_label(line: &str, label: &str) -> Option<u64> {
    let lower = line.to_ascii_lowercase();
    let label_pos = lower.find(label)?;
    let after = &line[label_pos + label.len()..];

    for token in after.split_whitespace().take(6) {
        let cleaned: String = token.chars().filter(|ch| ch.is_ascii_digit()).collect();
        if !cleaned.is_empty() {
            return cleaned.parse::<u64>().ok();
        }
    }

    None
}

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
        let lower = line.to_ascii_lowercase();
        if !lower.contains("token") {
            return Vec::new();
        }

        let input_tokens = parse_count_after_label(line, "input");
        let output_tokens = parse_count_after_label(line, "output");
        let cached_tokens = parse_count_after_label(line, "cached");

        let reported_tokens_total = input_tokens.unwrap_or(0) + output_tokens.unwrap_or(0) + cached_tokens.unwrap_or(0);
        if reported_tokens_total == 0 {
            return Vec::new();
        }

        vec![AgentParsedEvent {
            event_type: "token_report".into(),
            data: json!({
                "source": "copilot_cli_output",
                "line": line,
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "cached_tokens": cached_tokens,
                "reported_tokens_total": reported_tokens_total,
            }),
        }]
    }
}
