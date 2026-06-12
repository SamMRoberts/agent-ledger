use std::{collections::HashMap, path::Path};

use serde_json::json;

use crate::adapter::{
    binary_in_path, version_string, AgentAdapter, AgentDetection, AgentParsedEvent, CommandSpec,
};

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

fn parse_decimal_after_label(line: &str, label: &str) -> Option<f64> {
    let lower = line.to_ascii_lowercase();
    let label_pos = lower.find(label)?;
    let after = &line[label_pos + label.len()..];

    parse_first_decimal(after)
}

fn parse_first_decimal(text: &str) -> Option<f64> {
    for token in text.split_whitespace().take(8) {
        let cleaned: String = token
            .chars()
            .filter(|ch| ch.is_ascii_digit() || *ch == '.')
            .collect();
        if cleaned.chars().any(|ch| ch.is_ascii_digit()) {
            return cleaned.parse::<f64>().ok();
        }
    }

    None
}

fn parse_aic_used(line: &str) -> Option<f64> {
    parse_decimal_after_label(line, "aic used")
        .or_else(|| parse_decimal_after_label(line, "ai credits used"))
        .or_else(|| parse_decimal_after_label(line, "ai credit usage"))
        .or_else(|| parse_decimal_after_label(line, "this session"))
}

fn parse_aic_remaining(line: &str) -> Option<f64> {
    parse_decimal_after_label(line, "aic remaining")
        .or_else(|| parse_decimal_after_label(line, "ai credits remaining"))
        .or_else(|| parse_decimal_after_label(line, "remaining"))
}

fn parse_aic_limit(line: &str) -> Option<f64> {
    parse_decimal_after_label(line, "aic limit")
        .or_else(|| parse_decimal_after_label(line, "ai credits limit"))
        .or_else(|| parse_decimal_after_label(line, "limit"))
}

impl AgentAdapter for CopilotAdapter {
    fn name(&self) -> &'static str {
        "copilot"
    }

    fn detect(&self) -> anyhow::Result<AgentDetection> {
        let path = binary_in_path("copilot");
        let version = version_string("copilot", &["--version"])
            .or_else(|| version_string("copilot", &["version"]));
        let notes = vec![
            "Copilot CLI detection is based on the copilot binary being available in PATH".into(),
        ];
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
        let has_token_usage = lower.contains("token");
        let has_aic_usage = lower.contains("aic") || lower.contains("ai credit");
        if !has_token_usage && !has_aic_usage {
            return Vec::new();
        }

        let input_tokens = parse_count_after_label(line, "input");
        let output_tokens = parse_count_after_label(line, "output");
        let cached_tokens = parse_count_after_label(line, "cached");
        let aic_used = parse_aic_used(line);
        let aic_remaining = parse_aic_remaining(line);
        let aic_limit = parse_aic_limit(line);

        let reported_tokens_total =
            input_tokens.unwrap_or(0) + output_tokens.unwrap_or(0) + cached_tokens.unwrap_or(0);
        if reported_tokens_total == 0
            && aic_used.is_none()
            && aic_remaining.is_none()
            && aic_limit.is_none()
        {
            return Vec::new();
        }

        vec![AgentParsedEvent {
            event_type: "token_report".into(),
            data: json!({
                "source": if has_aic_usage { "copilot_aic_usage" } else { "copilot_cli_output" },
                "line": line,
                "report_kind": if has_aic_usage { "cumulative" } else { "delta" },
                "aic_used": aic_used,
                "aic_remaining": aic_remaining,
                "aic_limit": aic_limit,
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "cached_tokens": cached_tokens,
                "reported_tokens_total": reported_tokens_total,
            }),
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_copilot_aic_usage_line() {
        let adapter = CopilotAdapter;

        let events = adapter
            .parse_output_event("AI credits used this session: 1.25 remaining: 48.75 limit: 50");

        assert_eq!(events.len(), 1);
        let payload = &events[0].data;
        assert_eq!(events[0].event_type, "token_report");
        assert_eq!(payload["source"], "copilot_aic_usage");
        assert_eq!(payload["report_kind"], "cumulative");
        assert_eq!(payload["aic_used"], 1.25);
        assert_eq!(payload["aic_remaining"], 48.75);
        assert_eq!(payload["aic_limit"], 50.0);
    }

    #[test]
    fn parses_token_breakdown_line() {
        let adapter = CopilotAdapter;

        let events = adapter.parse_output_event("Tokens input: 1,000 output: 250 cached: 100");

        assert_eq!(events.len(), 1);
        let payload = &events[0].data;
        assert_eq!(payload["source"], "copilot_cli_output");
        assert_eq!(payload["reported_tokens_total"], 1350);
        assert_eq!(payload["input_tokens"], 1000);
        assert_eq!(payload["output_tokens"], 250);
        assert_eq!(payload["cached_tokens"], 100);
    }
}
