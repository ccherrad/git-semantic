use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

const SEMANTIC_BRANCH: &str = "semantic";
const INDEXED_AT_FILE: &str = ".indexed-at";
const INDEX_STATE_FILE: &str = ".index-state";
const DIR_CONTEXT_FILE: &str = ".dir-context";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StoredChunk {
    pub start_line: usize,
    pub end_line: usize,
    pub text: String,
    pub embedding: Vec<f32>,
}

pub enum FileChange {
    AddedOrModified(String),
    Deleted(String),
    Renamed { from: String, to: String },
}

pub fn read_last_indexed_sha(repo_path: &Path) -> Option<String> {
    let out = Command::new("git")
        .current_dir(repo_path)
        .args(["show", &format!("{}:{}", SEMANTIC_BRANCH, INDEXED_AT_FILE)])
        .output()
        .ok()?;

    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

pub fn get_changed_files(repo_path: &Path, since_sha: &str) -> Result<Vec<FileChange>> {
    let out = Command::new("git")
        .current_dir(repo_path)
        .args(["diff", "--name-status", "-M", since_sha, "HEAD"])
        .output()
        .context("Failed to run git diff")?;

    if !out.status.success() {
        anyhow::bail!("git diff failed: {}", String::from_utf8_lossy(&out.stderr));
    }

    let mut changes = Vec::new();

    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        match parts.as_slice() {
            [status, path] if status.starts_with('D') => {
                changes.push(FileChange::Deleted(path.to_string()));
            }
            [status, path]
                if status.starts_with('A')
                    || status.starts_with('M')
                    || status.starts_with('C') =>
            {
                changes.push(FileChange::AddedOrModified(path.to_string()));
            }
            [status, from, to] if status.starts_with('R') => {
                changes.push(FileChange::Renamed {
                    from: from.to_string(),
                    to: to.to_string(),
                });
            }
            _ => {}
        }
    }

    Ok(changes)
}

pub struct IndexSession {
    repo_path: PathBuf,
    worktree_path: PathBuf,
    already_indexed: HashSet<String>,
    dir_embeddings: HashMap<String, Vec<Vec<f32>>>,
}

impl IndexSession {
    pub fn open(repo_path: &Path, incremental: bool) -> Result<Self> {
        let worktree_path = repo_path.join(".git").join("semantic-worktree");

        if !incremental {
            ensure_semantic_branch(repo_path)?;
        }

        setup_worktree(repo_path, &worktree_path)?;

        let already_indexed = read_index_state(&worktree_path);

        Ok(Self {
            repo_path: repo_path.to_path_buf(),
            worktree_path,
            already_indexed,
            dir_embeddings: HashMap::new(),
        })
    }

    pub fn already_indexed(&self, relative_path: &str) -> bool {
        self.already_indexed.contains(relative_path)
    }

    pub fn has_partial_state(&self) -> bool {
        !self.already_indexed.is_empty()
    }

    pub fn write_file(&mut self, relative_path: &str, chunks: &[StoredChunk]) -> Result<()> {
        write_chunk_file(&self.worktree_path, relative_path, chunks)?;
        append_index_state(&self.worktree_path, relative_path)?;

        let dir = PathBuf::from(relative_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());

        let entry = self.dir_embeddings.entry(dir).or_default();
        for chunk in chunks {
            entry.push(chunk.embedding.clone());
        }

        Ok(())
    }

    pub fn delete_file(&self, relative_path: &str) -> Result<()> {
        let dest = self.worktree_path.join(relative_path);
        if dest.exists() {
            std::fs::remove_file(&dest)
                .with_context(|| format!("Failed to remove {}", relative_path))?;
        }
        Ok(())
    }

    pub fn commit(self) -> Result<()> {
        let mut all_chunks: Vec<(String, Vec<StoredChunk>)> = Vec::new();
        collect_chunks_from_dir(&self.worktree_path, &self.worktree_path, &mut all_chunks)?;

        let mut dir_embeddings: HashMap<String, Vec<Vec<f32>>> = HashMap::new();
        for (relative_path, chunks) in &all_chunks {
            let dir = PathBuf::from(relative_path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".to_string());
            let entry = dir_embeddings.entry(dir).or_default();
            for chunk in chunks {
                entry.push(chunk.embedding.clone());
            }
        }

        write_directory_centroids(&self.worktree_path, &dir_embeddings)?;
        write_indexed_at(&self.repo_path, &self.worktree_path)?;
        clear_index_state(&self.worktree_path)?;
        commit_worktree(&self.repo_path, &self.worktree_path)?;
        remove_worktree(&self.repo_path, &self.worktree_path)?;
        Ok(())
    }
}

fn read_index_state(worktree_path: &Path) -> HashSet<String> {
    let state_file = worktree_path.join(INDEX_STATE_FILE);
    match std::fs::read_to_string(state_file) {
        Ok(content) => content.lines().map(|l| l.to_string()).collect(),
        Err(_) => HashSet::new(),
    }
}

fn append_index_state(worktree_path: &Path, relative_path: &str) -> Result<()> {
    use std::io::Write;
    let state_file = worktree_path.join(INDEX_STATE_FILE);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state_file)
        .context("Failed to open .index-state")?;
    writeln!(file, "{}", relative_path).context("Failed to write .index-state")?;
    Ok(())
}

fn clear_index_state(worktree_path: &Path) -> Result<()> {
    let state_file = worktree_path.join(INDEX_STATE_FILE);
    if state_file.exists() {
        std::fs::remove_file(state_file).context("Failed to remove .index-state")?;
    }
    Ok(())
}

pub fn read_chunks_from_branch(repo_path: &Path) -> Result<Vec<(String, Vec<StoredChunk>)>> {
    let worktree_path = repo_path.join(".git").join("semantic-worktree");

    let fetch_result = Command::new("git")
        .current_dir(repo_path)
        .args([
            "fetch",
            "origin",
            &format!("{}:{}", SEMANTIC_BRANCH, SEMANTIC_BRANCH),
        ])
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
        anyhow::bail!("Semantic branch does not exist. Run `git-semantic index` first.");
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

        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name == ".git" || name == INDEXED_AT_FILE || name == INDEX_STATE_FILE || name == DIR_CONTEXT_FILE {
            continue;
        }

        if path.is_dir() {
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

fn write_chunk_file(
    worktree_path: &Path,
    relative_path: &str,
    chunks: &[StoredChunk],
) -> Result<()> {
    let dest = worktree_path.join(relative_path);

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create dirs for {}", relative_path))?;
    }

    let json = serde_json::to_string(chunks)
        .with_context(|| format!("Failed to serialize chunks for {}", relative_path))?;

    std::fs::write(&dest, json).with_context(|| format!("Failed to write {}", relative_path))?;

    Ok(())
}

fn write_indexed_at(repo_path: &Path, worktree_path: &Path) -> Result<()> {
    let head_sha = Command::new("git")
        .current_dir(repo_path)
        .args(["rev-parse", "HEAD"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    std::fs::write(worktree_path.join(INDEXED_AT_FILE), &head_sha)
        .context("Failed to write .indexed-at")?;

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
        let empty_tree = Command::new("git")
            .current_dir(repo_path)
            .args(["hash-object", "-t", "tree", "--stdin"])
            .stdin(std::process::Stdio::null())
            .output()
            .context("Failed to create empty tree")?;

        if !empty_tree.status.success() {
            anyhow::bail!(
                "Failed to create empty tree: {}",
                String::from_utf8_lossy(&empty_tree.stderr)
            );
        }

        let tree_sha = String::from_utf8_lossy(&empty_tree.stdout)
            .trim()
            .to_string();

        let commit = Command::new("git")
            .current_dir(repo_path)
            .args([
                "commit-tree",
                &tree_sha,
                "-m",
                "init: create semantic branch",
            ])
            .output()
            .context("Failed to create initial commit")?;

        if !commit.status.success() {
            anyhow::bail!(
                "Failed to create initial commit: {}",
                String::from_utf8_lossy(&commit.stderr)
            );
        }

        let commit_sha = String::from_utf8_lossy(&commit.stdout).trim().to_string();

        let out = Command::new("git")
            .current_dir(repo_path)
            .args(["branch", SEMANTIC_BRANCH, &commit_sha])
            .output()
            .context("Failed to create semantic branch")?;

        if !out.status.success() {
            anyhow::bail!(
                "Failed to create semantic branch: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }

    Ok(())
}

fn setup_worktree(repo_path: &Path, worktree_path: &Path) -> Result<()> {
    Command::new("git")
        .current_dir(repo_path)
        .args([
            "worktree",
            "remove",
            "--force",
            worktree_path.to_str().unwrap(),
        ])
        .output()
        .ok();
    if worktree_path.exists() {
        std::fs::remove_dir_all(worktree_path).ok();
    }
    Command::new("git")
        .current_dir(repo_path)
        .args(["worktree", "prune"])
        .output()
        .ok();

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

fn commit_worktree(repo_path: &Path, worktree_path: &Path) -> Result<()> {
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

fn remove_worktree(repo_path: &Path, worktree_path: &Path) -> Result<()> {
    Command::new("git")
        .current_dir(repo_path)
        .args([
            "worktree",
            "remove",
            "--force",
            worktree_path.to_str().unwrap(),
        ])
        .output()
        .context("Failed to remove worktree")?;

    Ok(())
}

fn compute_centroid(embeddings: &[Vec<f32>]) -> Vec<f32> {
    if embeddings.is_empty() {
        return vec![];
    }
    let dim = embeddings[0].len();
    let mut centroid = vec![0.0f32; dim];
    for emb in embeddings {
        for (i, v) in emb.iter().enumerate() {
            centroid[i] += v;
        }
    }
    let n = embeddings.len() as f32;
    centroid.iter_mut().for_each(|v| *v /= n);
    centroid
}

fn write_directory_centroids(
    worktree_path: &Path,
    dir_embeddings: &HashMap<String, Vec<Vec<f32>>>,
) -> Result<()> {
    use crate::models::DirectoryCentroid;

    let propagated = propagate_centroids(dir_embeddings);

    for (dir, embeddings) in &propagated {
        if embeddings.is_empty() {
            continue;
        }

        let centroid = DirectoryCentroid {
            dir_path: dir.clone(),
            centroid: compute_centroid(embeddings),
            chunk_count: embeddings.len(),
        };

        let json = serde_json::to_string(&centroid)
            .context("Failed to serialize centroid")?;

        let dir_path = if dir == "." {
            worktree_path.to_path_buf()
        } else {
            worktree_path.join(dir)
        };

        std::fs::create_dir_all(&dir_path)
            .with_context(|| format!("Failed to create dir {:?}", dir_path))?;

        std::fs::write(dir_path.join(DIR_CONTEXT_FILE), json)
            .with_context(|| format!("Failed to write .dir-context for {}", dir))?;
    }

    Ok(())
}

fn propagate_centroids(
    dir_embeddings: &HashMap<String, Vec<Vec<f32>>>,
) -> HashMap<String, Vec<Vec<f32>>> {
    let mut propagated: HashMap<String, Vec<Vec<f32>>> = HashMap::new();

    for (dir, embeddings) in dir_embeddings {
        // Add embeddings to every ancestor directory up to root
        let mut path = PathBuf::from(dir);
        loop {
            propagated
                .entry(path.to_string_lossy().to_string())
                .or_default()
                .extend(embeddings.iter().cloned());

            match path.parent() {
                Some(parent) if parent != path => {
                    path = parent.to_path_buf();
                }
                _ => break,
            }
        }
    }

    propagated
}

pub fn build_and_write_centroids(repo_path: &Path) -> Result<usize> {
    let worktree_path = repo_path.join(".git").join("semantic-worktree");
    setup_worktree(repo_path, &worktree_path)?;

    let mut dir_embeddings: HashMap<String, Vec<Vec<f32>>> = HashMap::new();
    let mut all_chunks: Vec<(String, Vec<StoredChunk>)> = Vec::new();
    collect_chunks_from_dir(&worktree_path, &worktree_path, &mut all_chunks)?;

    for (relative_path, chunks) in &all_chunks {
        let dir = PathBuf::from(relative_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        let entry = dir_embeddings.entry(dir).or_default();
        for chunk in chunks {
            entry.push(chunk.embedding.clone());
        }
    }

    let n = dir_embeddings.len();
    write_directory_centroids(&worktree_path, &dir_embeddings)?;

    Command::new("git")
        .current_dir(&worktree_path)
        .args(["add", DIR_CONTEXT_FILE])
        .output()
        .context("Failed to stage .dir-context")?;

    let status = Command::new("git")
        .current_dir(&worktree_path)
        .args(["diff", "--cached", "--quiet"])
        .status()
        .context("Failed to check worktree status")?;

    if !status.success() {
        Command::new("git")
            .current_dir(&worktree_path)
            .args(["commit", "-m", "index: build directory centroids"])
            .output()
            .context("Failed to commit .dir-context")?;
    }

    remove_worktree(repo_path, &worktree_path)?;
    Ok(n)
}

pub fn read_centroids_from_branch(repo_path: &Path) -> Result<Vec<crate::models::DirectoryCentroid>> {
    let worktree_path = repo_path.join(".git").join("semantic-worktree");
    setup_worktree(repo_path, &worktree_path)?;

    let mut centroids = Vec::new();
    collect_centroids_from_dir(&worktree_path, &worktree_path, &mut centroids);

    remove_worktree(repo_path, &worktree_path)?;
    Ok(centroids)
}

fn collect_centroids_from_dir(
    base: &Path,
    dir: &Path,
    result: &mut Vec<crate::models::DirectoryCentroid>,
) {
    let context_file = dir.join(DIR_CONTEXT_FILE);
    if context_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&context_file) {
            if let Ok(centroid) = serde_json::from_str::<crate::models::DirectoryCentroid>(&content) {
                result.push(centroid);
            }
        }
    }

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if path.is_dir() && name != ".git" {
                collect_centroids_from_dir(base, &path, result);
            }
        }
    }
}
