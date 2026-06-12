use std::{path::Path, process::Command};

use anyhow::{anyhow, Context};

fn run_git(repo_dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(args)
        .output()
        .with_context(|| format!("running git {:?} in {}", args, repo_dir.display()))?;
    if !output.status.success() {
        return Err(anyhow!(String::from_utf8_lossy(&output.stderr).trim().to_string()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn get_current_commit(repo_dir: &Path) -> anyhow::Result<String> {
    run_git(repo_dir, &["rev-parse", "HEAD"])
}

pub fn get_diff(repo_dir: &Path) -> anyhow::Result<String> {
    run_git(repo_dir, &["--no-pager", "diff", "HEAD"])
}

pub fn get_status(repo_dir: &Path) -> anyhow::Result<String> {
    run_git(repo_dir, &["status", "--short"])
}

pub fn create_bundle(repo_dir: &Path, output_path: &Path) -> anyhow::Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["bundle", "create"])
        .arg(output_path)
        .arg("--all")
        .output()
        .with_context(|| format!("creating git bundle in {}", repo_dir.display()))?;
    if !output.status.success() {
        return Err(anyhow!(String::from_utf8_lossy(&output.stderr).trim().to_string()));
    }
    Ok(())
}

pub fn is_git_repo(dir: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}
