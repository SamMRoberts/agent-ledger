use std::fs;

use agent_ledger_core::manifest::ChallengeManifest;

use super::sessions_dir;

pub async fn run() -> anyhow::Result<()> {
    if super::ledger_dir().exists() {
        anyhow::bail!(".ledger already exists")
    }

    fs::create_dir_all(sessions_dir())?;
    let manifest = ChallengeManifest::default_manifest();
    fs::write("ledger.yaml", serde_yaml::to_string(&manifest)?)?;
    println!("Initialized agent-ledger in {}", super::ledger_dir().display());
    Ok(())
}
