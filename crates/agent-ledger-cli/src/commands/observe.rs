use std::{
    path::{Path, PathBuf},
    sync::mpsc::TryRecvError,
    time::{Duration, Instant},
};

use agent_ledger_core::{event::EventType, session::SessionStatus};
use agent_ledger_runner::file_watcher::FileWatcher;
use serde_json::json;

use super::session_lifecycle::{
    capture_session_snapshot, create_session, finish_session, observer_evidence_capture,
    SessionContext,
};

#[derive(Debug, Clone)]
struct ObservedFileEvent {
    event_type: EventType,
    payload: serde_json::Value,
}

pub async fn run(
    agent: String,
    snapshot_interval_seconds: u64,
    duration_seconds: Option<u64>,
) -> anyhow::Result<()> {
    let snapshot_interval = Duration::from_secs(snapshot_interval_seconds.max(1));
    let max_duration = duration_seconds.map(Duration::from_secs);
    let context = create_session(agent, observer_evidence_capture())?;
    let workspace_dir = context.workspace_dir.clone();
    let session_id = context.session.id.clone();

    println!("Observing session {}", session_id);
    println!("Run your agent normally, then press Ctrl-C to finish observation.");

    let (tx, rx) = std::sync::mpsc::channel();
    let _watcher = FileWatcher::new(&workspace_dir, tx)?;
    let started_at = Instant::now();
    let mut next_snapshot = tokio::time::interval(snapshot_interval);
    next_snapshot.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    next_snapshot.tick().await;

    loop {
        drain_file_events(&context, &workspace_dir, &rx)?;

        if max_duration.is_some_and(|duration| started_at.elapsed() >= duration) {
            break;
        }

        tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal?;
                break;
            }
            _ = next_snapshot.tick() => {
                capture_session_snapshot(&context, "observe")?;
            }
            _ = tokio::time::sleep(Duration::from_millis(250)) => {}
        }
    }

    drain_file_events(&context, &workspace_dir, &rx)?;
    let final_workspace_hash = capture_session_snapshot(&context, "observe_finish")?;
    finish_session(
        context,
        SessionStatus::Finished,
        final_workspace_hash.total_hash,
        json!({
            "mode": "observe",
            "duration_seconds": started_at.elapsed().as_secs(),
        }),
    )?;

    println!("Finished observing session {}", session_id);
    Ok(())
}

fn drain_file_events<T>(
    context: &SessionContext,
    root: &Path,
    rx: &std::sync::mpsc::Receiver<T>,
) -> anyhow::Result<()>
where
    T: std::fmt::Debug,
{
    loop {
        match rx.try_recv() {
            Ok(event) => {
                for observed in observed_events_from_debug(root, &event) {
                    let mut event_log = context
                        .event_log
                        .lock()
                        .map_err(|_| anyhow::anyhow!("event log mutex poisoned"))?;
                    event_log.append(observed.event_type, observed.payload)?;
                }
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => break,
        }
    }
    Ok(())
}

fn observed_events_from_debug<T: std::fmt::Debug>(
    root: &Path,
    event: &T,
) -> Vec<ObservedFileEvent> {
    let description = format!("{event:?}");
    let event_type = classify_event_debug(&description);
    let Some(event_type) = event_type else {
        return Vec::new();
    };

    extract_paths_from_debug(root, &description)
        .into_iter()
        .filter(|path| !is_ignored_relative_path(path))
        .map(|path| ObservedFileEvent {
            event_type: event_type.clone(),
            payload: json!({
                "path": path,
                "source": "workspace_watcher",
            }),
        })
        .collect()
}

fn classify_event_debug(description: &str) -> Option<EventType> {
    if description.contains("Rename") && description.contains("Modify") {
        Some(EventType::FileRenamed)
    } else if description.contains("Create") {
        Some(EventType::FileCreated)
    } else if description.contains("Remove") {
        Some(EventType::FileDeleted)
    } else if description.contains("Modify") {
        Some(EventType::FileModified)
    } else {
        None
    }
}

fn extract_paths_from_debug(root: &Path, description: &str) -> Vec<String> {
    let marker = "paths: [";
    let Some(paths_start) = description.find(marker).map(|index| index + marker.len()) else {
        return Vec::new();
    };
    let Some(paths_end) = description[paths_start..].find(']') else {
        return Vec::new();
    };
    let paths = &description[paths_start..paths_start + paths_end];

    paths
        .split(',')
        .filter_map(|part| {
            let trimmed = part
                .trim()
                .trim_matches(|char| matches!(char, '"' | '\\' | '/' | ' '));
            if trimmed.is_empty() {
                return None;
            }
            let absolute = if root.is_absolute() {
                format!("/{}", trimmed.trim_start_matches('/'))
            } else {
                trimmed.to_owned()
            };
            let path = PathBuf::from(absolute);
            let relative = path.strip_prefix(root).unwrap_or(&path);
            Some(relative.to_string_lossy().replace('\\', "/"))
        })
        .collect()
}

fn is_ignored_relative_path(path: &str) -> bool {
    path.split('/').any(|component| {
        matches!(
            component,
            ".git" | ".ledger" | "target" | "node_modules" | "dist" | "build"
        )
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    struct FakeEvent(&'static str);

    impl std::fmt::Debug for FakeEvent {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(self.0)
        }
    }

    #[test]
    fn classifies_create_events_from_debug_text() {
        let root = Path::new("/repo");
        let event = FakeEvent(
            "Event { kind: Create(File), paths: [\"/repo/src/main.rs\"], attrs: Tracker }",
        );

        let observed = observed_events_from_debug(root, &event);

        assert_eq!(observed.len(), 1);
        assert_eq!(observed[0].event_type, EventType::FileCreated);
        assert_eq!(observed[0].payload["path"], "src/main.rs");
    }

    #[test]
    fn filters_internal_and_generated_paths() {
        let root = Path::new("/repo");
        let event = FakeEvent(
            "Event { kind: Modify(Data(Any)), paths: [\"/repo/.ledger/events.jsonl\", \"/repo/target/debug/app\", \"/repo/src/lib.rs\"], attrs: Tracker }",
        );

        let observed = observed_events_from_debug(root, &event);

        assert_eq!(observed.len(), 1);
        assert_eq!(observed[0].event_type, EventType::FileModified);
        assert_eq!(observed[0].payload["path"], "src/lib.rs");
    }

    #[test]
    fn ignores_events_without_supported_kind() {
        let root = Path::new("/repo");
        let event =
            FakeEvent("Event { kind: Access(Close(Write)), paths: [\"/repo/src/lib.rs\"] }");

        let observed = observed_events_from_debug(root, &event);

        assert!(observed.is_empty());
    }
}
