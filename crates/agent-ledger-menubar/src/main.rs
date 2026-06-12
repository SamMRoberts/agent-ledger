use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

mod presentation;

#[cfg(target_os = "macos")]
mod macos;

#[derive(Debug, Clone, Parser)]
#[command(name = "agent-ledger-menubar", about = "macOS menubar status surface for agent-ledger")]
struct Args {
    #[arg(long, default_value = ".")]
    root: PathBuf,
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u64).range(1..=300))]
    refresh_seconds: u64,
}

fn main() -> Result<()> {
    let args = Args::parse();

    #[cfg(target_os = "macos")]
    {
        return macos::run(args);
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = args;
        anyhow::bail!("agent-ledger-menubar is only supported on macOS")
    }
}
