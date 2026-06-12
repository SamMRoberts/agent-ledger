pub mod adapter;
pub mod claude;
pub mod codex;
pub mod copilot;

pub use adapter::{AgentAdapter, AgentDetection, AgentParsedEvent, CommandSpec};

pub fn get_adapter(name: &str) -> Option<Box<dyn AgentAdapter>> {
    match name {
        "copilot" => Some(Box::new(copilot::CopilotAdapter)),
        "codex" => Some(Box::new(codex::CodexAdapter)),
        "claude" => Some(Box::new(claude::ClaudeAdapter)),
        _ => None,
    }
}

pub fn list_adapters() -> Vec<&'static str> {
    vec!["copilot", "codex", "claude"]
}
