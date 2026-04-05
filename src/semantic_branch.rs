use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

const SEMANTIC_BRANCH: &str = "semantic";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StoredChunk {
    pub start_line: usize,
    pub end_line: usize,
    pub text: String,
    pub embedding: Vec<f32>,
}

pub fn write_chunks_to_branch(
    repo_path: &Path,
    file_chunks: &[(String, Vec<StoredChunk>)],
) -> Result<()> {
    let worktree_path = repo_path.join(".git").join("semantic-worktree");

    ensure_semantic_branch(repo_path)?;
    setup_worktree(repo_path, &worktree_path)?;

    for (relative_path, chunks) in file_chunks {
        let dest = worktree_path.join(relative_path);

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create dirs for {}", relative_path))?;
        }

        let json = serde_json::to_string(chunks)
            .with_context(|| format!("Failed to serialize chunks for {}", relative_path))?;

        std::fs::write(&dest, json)
            .with_context(|| format!("Failed to write {}", relative_path))?;
    }

    commit_worktree(repo_path, &worktree_path)?;
    remove_worktree(repo_path, &worktree_path)?;

    Ok(())
}

pub fn read_chunks_from_branch(repo_path: &Path) -> Result<Vec<(String, Vec<StoredChunk>)>> {
    let worktree_path = repo_path.join(".git").join("semantic-worktree");

    let fetch_result = Command::new("git")
        .current_dir(repo_path)
        .args(["fetch", "origin", &format!("{}:{}", SEMANTIC_BRANCH, SEMANTIC_BRANCH)])
        .output();

    if let Ok(out) = fetch_result {
        if !out.status.success() {
            println!("  (no remote semantic branch, using local)");
        }
    }

    let branch_exists = Command::new("git")
        .current_dir(repo_path)
        .args(["rev-parse", "--verify", SEMANTIC_BRANCH])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !branch_exists {
        anyhow::bail!("Semantic branch does not exist. Run `gitsem index` first.");
    }

    setup_worktree(repo_path, &worktree_path)?;

    let mut result = Vec::new();
    collect_chunks_from_dir(&worktree_path, &worktree_path, &mut result)?;

    remove_worktree(repo_path, &worktree_path)?;

    Ok(result)
}

fn collect_chunks_from_dir(
    base: &Path,
    dir: &Path,
    result: &mut Vec<(String, Vec<StoredChunk>)>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("Failed to read dir {:?}", dir))? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name == ".git" {
                continue;
            }
            collect_chunks_from_dir(base, &path, result)?;
        } else {
            let relative = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();

            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", relative))?;

            let chunks: Vec<StoredChunk> = serde_json::from_str(&content)
                .with_context(|| format!("Failed to parse {}", relative))?;

            result.push((relative, chunks));
        }
    }

    Ok(())
}

fn ensure_semantic_branch(repo_path: &Path) -> Result<()> {
    let exists = Command::new("git")
        .current_dir(repo_path)
        .args(["rev-parse", "--verify", SEMANTIC_BRANCH])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !exists {
        let out = Command::new("git")
            .current_dir(repo_path)
            .args(["checkout", "--orphan", SEMANTIC_BRANCH])
            .output()
            .context("Failed to create orphan branch")?;

        if !out.status.success() {
            anyhow::bail!(
                "Failed to create semantic branch: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }

        Command::new("git")
            .current_dir(repo_path)
            .args(["rm", "-rf", "--cached", "."])
            .output()
            .ok();

        Command::new("git")
            .current_dir(repo_path)
            .args(["checkout", "-"])
            .output()
            .context("Failed to return to original branch")?;
    }

    Ok(())
}

fn setup_worktree(repo_path: &Path, worktree_path: &PathBuf) -> Result<()> {
    if worktree_path.exists() {
        Command::new("git")
            .current_dir(repo_path)
            .args(["worktree", "remove", "--force", worktree_path.to_str().unwrap()])
            .output()
            .ok();
    }

    let out = Command::new("git")
        .current_dir(repo_path)
        .args([
            "worktree",
            "add",
            "--no-checkout",
            worktree_path.to_str().unwrap(),
            SEMANTIC_BRANCH,
        ])
        .output()
        .context("Failed to add worktree")?;

    if !out.status.success() {
        anyhow::bail!(
            "Failed to set up worktree: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let out = Command::new("git")
        .current_dir(worktree_path)
        .args(["checkout", SEMANTIC_BRANCH, "--", "."])
        .output();

    if let Ok(o) = out {
        if !o.status.success() {
            // Branch is empty (first index), that's fine
        }
    }

    Ok(())
}

fn commit_worktree(repo_path: &Path, worktree_path: &PathBuf) -> Result<()> {
    Command::new("git")
        .current_dir(worktree_path)
        .args(["add", "-A"])
        .output()
        .context("Failed to stage files in worktree")?;

    let status = Command::new("git")
        .current_dir(worktree_path)
        .args(["diff", "--cached", "--quiet"])
        .status()
        .context("Failed to check worktree status")?;

    if status.success() {
        println!("  Semantic branch already up to date.");
        return Ok(());
    }

    let head_sha = Command::new("git")
        .current_dir(repo_path)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let message = format!("index: update embeddings from {}", head_sha);

    let out = Command::new("git")
        .current_dir(worktree_path)
        .args(["commit", "-m", &message])
        .output()
        .context("Failed to commit to semantic branch")?;

    if !out.status.success() {
        anyhow::bail!(
            "Failed to commit semantic branch: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    Ok(())
}

fn remove_worktree(repo_path: &Path, worktree_path: &PathBuf) -> Result<()> {
    Command::new("git")
        .current_dir(repo_path)
        .args(["worktree", "remove", "--force", worktree_path.to_str().unwrap()])
        .output()
        .context("Failed to remove worktree")?;

    Ok(())
}
