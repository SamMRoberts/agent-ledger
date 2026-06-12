use std::{collections::HashMap, io::Read};

use agent_ledger_core::{
    event::Event,
    hash_chain::verify_chain,
    signing::verify_signature,
    workspace::WorkspaceHash,
};
use anyhow::anyhow;
use ed25519_dalek::VerifyingKey;
use flate2::read::GzDecoder;
use tar::Archive;

use super::{required_file, SessionManifestFile};

fn load_events_from_bytes(bytes: &[u8]) -> anyhow::Result<Vec<Event>> {
    let content = std::str::from_utf8(bytes)?;
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub async fn run(bundle_path: std::path::PathBuf) -> anyhow::Result<()> {
    let file = std::fs::File::open(&bundle_path)?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    let mut files: HashMap<String, Vec<u8>> = HashMap::new();

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_string_lossy().to_string();
        let mut contents = Vec::new();
        entry.read_to_end(&mut contents)?;
        files.insert(path, contents);
    }

    let manifest: SessionManifestFile = serde_json::from_slice(required_file(&files, "session_manifest.json")?)?;
    let events = load_events_from_bytes(required_file(&files, "events.jsonl")?)?;
    verify_chain(&events)?;

    let workspace_hash: WorkspaceHash = serde_json::from_slice(required_file(&files, "workspace_hash.json")?)?;
    let expected_workspace_hash = manifest
        .session
        .final_workspace_hash
        .clone()
        .ok_or_else(|| anyhow!("session manifest missing final workspace hash"))?;
    if workspace_hash.total_hash != expected_workspace_hash {
        anyhow::bail!("workspace hash mismatch")
    }

    let events_hash = events.last().map(|event| event.event_hash.clone()).unwrap_or_else(|| "genesis".into());
    let signing_input = format!("{}{}{}", manifest.session.id, workspace_hash.total_hash, events_hash);
    let digest = blake3::hash(signing_input.as_bytes());

    let public_key_bytes: [u8; 32] = hex::decode(manifest.public_key_hex.trim())?
        .try_into()
        .map_err(|_| anyhow!("invalid public key length"))?;
    let public_key = VerifyingKey::from_bytes(&public_key_bytes)?;

    let signature_bytes = hex::decode(std::str::from_utf8(required_file(&files, "signature.ed25519")?)?.trim())?;
    verify_signature(&public_key, digest.as_bytes(), &signature_bytes)?;

    println!("Bundle verification passed for {}", manifest.session.id);
    Ok(())
}
