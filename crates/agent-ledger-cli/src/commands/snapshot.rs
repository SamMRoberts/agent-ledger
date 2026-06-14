use super::{
    active_or_latest_session_dir, read_session_manifest,
    session_lifecycle::{capture_session_snapshot, SessionContext},
};

pub async fn run() -> anyhow::Result<()> {
    let session_dir =
        active_or_latest_session_dir()?.ok_or_else(|| anyhow::anyhow!("no sessions found"))?;
    let session_manifest = read_session_manifest(&super::session_manifest_path(&session_dir))?;
    let context = SessionContext::for_existing(session_manifest, session_dir)?;
    capture_session_snapshot(&context, "snapshot")?;

    println!("Snapshot captured for {}", context.session.id);
    Ok(())
}
