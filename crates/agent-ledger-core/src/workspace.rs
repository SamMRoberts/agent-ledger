use std::{fs, path::Path, time::SystemTime};

use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use walkdir::{DirEntry, WalkDir};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub hash: String,
    pub modified_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceHash {
    pub entries: Vec<FileEntry>,
    pub total_hash: String,
    pub computed_at: DateTime<Utc>,
}

fn is_excluded(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return false;
    }
    matches!(
        entry.file_name().to_string_lossy().as_ref(),
        ".git" | ".ledger" | "node_modules" | "target" | "dist" | "build"
    )
}

pub fn compute_workspace_hash(dir: &Path) -> anyhow::Result<WorkspaceHash> {
    let mut entries = Vec::new();
    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_entry(|entry| !is_excluded(entry))
    {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let contents = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let metadata = entry.metadata()?;
        let modified_at = metadata
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let relative_path = path
            .strip_prefix(dir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        entries.push(FileEntry {
            path: relative_path,
            size: metadata.len(),
            hash: hex::encode(blake3::hash(&contents).as_bytes()),
            modified_at: DateTime::<Utc>::from(modified_at),
        });
    }

    entries.sort_by(|left, right| left.path.cmp(&right.path));

    let mut hasher = blake3::Hasher::new();
    for entry in &entries {
        hasher.update(entry.path.as_bytes());
        hasher.update(&[0]);
        hasher.update(entry.hash.as_bytes());
        hasher.update(&[0]);
        hasher.update(entry.size.to_string().as_bytes());
        hasher.update(&[0]);
    }

    Ok(WorkspaceHash {
        entries,
        total_hash: hex::encode(hasher.finalize().as_bytes()),
        computed_at: Utc::now(),
    })
}

impl WorkspaceHash {
    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use uuid::Uuid;

    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::current_dir()
            .expect("current dir")
            .join(".agent-ledger-test-artifacts")
            .join(format!("{name}-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    #[test]
    fn same_files_produce_same_hash() {
        let dir_a = test_dir("same-a");
        let dir_b = test_dir("same-b");
        fs::write(dir_a.join("file.txt"), "hello world").expect("write file a");
        fs::write(dir_b.join("file.txt"), "hello world").expect("write file b");

        let hash_a = compute_workspace_hash(&dir_a).expect("hash a");
        let hash_b = compute_workspace_hash(&dir_b).expect("hash b");
        assert_eq!(hash_a.total_hash, hash_b.total_hash);

        fs::remove_dir_all(dir_a).expect("cleanup dir a");
        fs::remove_dir_all(dir_b).expect("cleanup dir b");
    }

    #[test]
    fn different_files_produce_different_hashes() {
        let dir_a = test_dir("different-a");
        let dir_b = test_dir("different-b");
        fs::write(dir_a.join("file.txt"), "hello world").expect("write file a");
        fs::write(dir_b.join("file.txt"), "different content").expect("write file b");

        let hash_a = compute_workspace_hash(&dir_a).expect("hash a");
        let hash_b = compute_workspace_hash(&dir_b).expect("hash b");
        assert_ne!(hash_a.total_hash, hash_b.total_hash);

        fs::remove_dir_all(dir_a).expect("cleanup dir a");
        fs::remove_dir_all(dir_b).expect("cleanup dir b");
    }

    #[test]
    fn renamed_files_produce_different_hashes() {
        let dir_a = test_dir("renamed-a");
        let dir_b = test_dir("renamed-b");
        fs::write(dir_a.join("file-a.txt"), "same content").expect("write file a");
        fs::write(dir_b.join("file-b.txt"), "same content").expect("write file b");

        let hash_a = compute_workspace_hash(&dir_a).expect("hash a");
        let hash_b = compute_workspace_hash(&dir_b).expect("hash b");

        assert_ne!(hash_a.total_hash, hash_b.total_hash);

        fs::remove_dir_all(dir_a).expect("cleanup dir a");
        fs::remove_dir_all(dir_b).expect("cleanup dir b");
    }

    #[test]
    fn excluded_dirs_are_ignored() {
        let dir = test_dir("excluded");
        fs::write(dir.join("file.txt"), "stable").expect("write root file");
        fs::create_dir_all(dir.join("node_modules/pkg")).expect("create excluded dir");
        fs::write(dir.join("node_modules/pkg/index.js"), "first").expect("write excluded file");

        let hash_a = compute_workspace_hash(&dir).expect("hash a");
        fs::write(dir.join("node_modules/pkg/index.js"), "second").expect("rewrite excluded file");
        let hash_b = compute_workspace_hash(&dir).expect("hash b");

        assert_eq!(hash_a.total_hash, hash_b.total_hash);
        assert!(hash_a.entries.iter().all(|entry| !entry.path.starts_with("node_modules/")));

        fs::remove_dir_all(dir).expect("cleanup dir");
    }
}
