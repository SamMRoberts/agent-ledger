use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use agent_ledger_agents::{get_adapter, AgentAdapter, AgentParsedEvent};
use agent_ledger_core::{
    event::{EventLog, EventType},
    session::SessionStatus,
};
use agent_ledger_runner::process::{AgentLineHandler, ProcessRunner};
use serde_json::json;

use super::{
    evidence_capture_from_command_spec,
    session_lifecycle::{capture_session_snapshot, create_session, finish_session},
};

fn parsed_event_type(parsed: &AgentParsedEvent) -> Option<EventType> {
    match parsed.event_type.as_str() {
        "token_report" => Some(EventType::TokenReport),
        "warning" => Some(EventType::Warning),
        "error" => Some(EventType::Error),
        _ => None,
    }
}

fn append_parsed_agent_line_events(
    adapter: &dyn AgentAdapter,
    line: &str,
    event_log: &Arc<Mutex<EventLog>>,
) -> anyhow::Result<()> {
    let parsed_events = adapter.parse_output_event(line);
    if parsed_events.is_empty() {
        return Ok(());
    }

    let mut log = event_log
        .lock()
        .map_err(|_| anyhow::anyhow!("event log mutex poisoned"))?;
    for parsed in parsed_events {
        if let Some(event_type) = parsed_event_type(&parsed) {
            log.append(event_type, parsed.data)?;
        }
    }

    Ok(())
}

pub async fn run(agent: String) -> anyhow::Result<()> {
    let adapter: Arc<dyn AgentAdapter> = Arc::from(
        get_adapter(&agent).ok_or_else(|| anyhow::anyhow!("unsupported agent: {agent}"))?,
    );
    let detection = adapter.detect()?;
    if !detection.found {
        anyhow::bail!("agent '{}' is not available in PATH", agent)
    }
    let spec = adapter.launch_command(Path::new("."))?;
    let evidence_capture = evidence_capture_from_command_spec(&spec);

    let context = create_session(agent, evidence_capture)?;
    let event_log = Arc::clone(&context.event_log);
    let line_handler: AgentLineHandler = {
        let adapter = Arc::clone(&adapter);
        let event_log = Arc::clone(&event_log);
        Arc::new(move |_event_type, line| {
            append_parsed_agent_line_events(adapter.as_ref(), line, &event_log)
        })
    };
    let runner = ProcessRunner::new(context.session.id.to_string(), Arc::clone(&event_log))
        .with_line_handler(line_handler);
    let exit_code = runner.run_agent(&spec, &context.workspace_dir).await?;

    let final_workspace_hash = capture_session_snapshot(&context, "finish")?;
    let final_status = if exit_code == 0 {
        SessionStatus::Finished
    } else {
        SessionStatus::Failed
    };
    let session_id = context.session.id.clone();
    finish_session(
        context,
        final_status,
        final_workspace_hash.total_hash,
        json!({ "exit_code": exit_code }),
    )?;

    println!("Completed session {}", session_id);
    Ok(())
}
