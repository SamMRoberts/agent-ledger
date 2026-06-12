use std::process::Command;

use agent_ledger_agents::{get_adapter, list_adapters};

pub async fn run() -> anyhow::Result<()> {
    let git_ok = Command::new("git").arg("--version").output().map(|output| output.status.success()).unwrap_or(false);
    println!("git: {}", if git_ok { "ok" } else { "missing" });

    for name in list_adapters() {
        if let Some(adapter) = get_adapter(name) {
            let detection = adapter.detect()?;
            println!("{}: {}", adapter.name(), if detection.found { "ok" } else { "missing" });
            if let Some(version) = detection.version {
                println!("  version: {version}");
            }
            if let Some(path) = detection.path {
                println!("  path: {}", path.display());
            }
            for note in detection.notes {
                println!("  note: {note}");
            }
        }
    }

    Ok(())
}
