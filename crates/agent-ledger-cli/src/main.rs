use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod commands;

#[derive(Parser)]
#[command(
    name = "agent-ledger",
    about = "Tamper-evident coding challenge session runner"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Doctor,
    Init,
    Start {
        #[arg(long)]
        agent: String,
    },
    Observe {
        #[arg(long)]
        agent: String,
        #[arg(long, default_value_t = 300)]
        snapshot_interval_seconds: u64,
        #[arg(long)]
        duration_seconds: Option<u64>,
    },
    Snapshot,
    Status,
    Submit,
    Verify {
        bundle_path: std::path::PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Doctor => commands::doctor::run().await?,
        Commands::Init => commands::init::run().await?,
        Commands::Start { agent } => commands::start::run(agent).await?,
        Commands::Observe {
            agent,
            snapshot_interval_seconds,
            duration_seconds,
        } => commands::observe::run(agent, snapshot_interval_seconds, duration_seconds).await?,
        Commands::Snapshot => commands::snapshot::run().await?,
        Commands::Status => commands::status::run().await?,
        Commands::Submit => commands::submit::run().await?,
        Commands::Verify { bundle_path } => commands::verify::run(bundle_path).await?,
    }
    Ok(())
}
