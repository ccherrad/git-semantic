use anyhow::{Context, Result};
use crate::models::CodeChunk;
use std::path::Path;
use std::process::Command;

const SEMANTIC_NOTES_REF: &str = "refs/notes/semantic";

pub fn write_note(repo_path: &Path, commit_sha: &str, chunks: &[CodeChunk]) -> Result<()> {
    let note_content = serde_json::to_string_pretty(&chunks)
        .context("Failed to serialize chunks to JSON")?;

    let temp_file = repo_path.join(".git").join("semantic-note-temp.json");
    std::fs::write(&temp_file, &note_content)
        .context("Failed to write temporary note file")?;

    let output = Command::new("git")
        .current_dir(repo_path)
        .args([
            "notes",
            "--ref",
            SEMANTIC_NOTES_REF,
            "add",
            "-f",
            "-F",
            temp_file.to_str().unwrap(),
            commit_sha,
        ])
        .output()
        .context("Failed to execute git notes add")?;

    std::fs::remove_file(&temp_file).ok();

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to add git note: {}", stderr);
    }

    Ok(())
}

pub fn read_notes(repo_path: &Path) -> Result<Vec<(String, Vec<CodeChunk>)>> {
    let output = Command::new("git")
        .current_dir(repo_path)
        .args([
            "for-each-ref",
            "--format=%(refname:short)",
            SEMANTIC_NOTES_REF,
        ])
        .output()
        .context("Failed to check if notes ref exists")?;

    if !output.status.success() || output.stdout.is_empty() {
        return Ok(Vec::new());
    }

    let list_output = Command::new("git")
        .current_dir(repo_path)
        .args([
            "notes",
            "--ref",
            SEMANTIC_NOTES_REF,
            "list",
        ])
        .output()
        .context("Failed to list git notes")?;

    if !list_output.status.success() {
        return Ok(Vec::new());
    }

    let notes_list = String::from_utf8_lossy(&list_output.stdout);
    let mut all_notes = Vec::new();

    for line in notes_list.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 2 {
            continue;
        }

        let commit_sha = parts[1];

        let show_output = Command::new("git")
            .current_dir(repo_path)
            .args([
                "notes",
                "--ref",
                SEMANTIC_NOTES_REF,
                "show",
                commit_sha,
            ])
            .output()
            .context("Failed to show git note")?;

        if !show_output.status.success() {
            continue;
        }

        let note_content = String::from_utf8_lossy(&show_output.stdout);

        match serde_json::from_str::<Vec<CodeChunk>>(&note_content) {
            Ok(chunks) => {
                all_notes.push((commit_sha.to_string(), chunks));
            }
            Err(e) => {
                eprintln!("Warning: Failed to parse note for commit {}: {}", commit_sha, e);
                continue;
            }
        }
    }

    Ok(all_notes)
}

pub fn list_commits_with_notes(repo_path: &Path) -> Result<Vec<String>> {
    let list_output = Command::new("git")
        .current_dir(repo_path)
        .args([
            "notes",
            "--ref",
            SEMANTIC_NOTES_REF,
            "list",
        ])
        .output()
        .context("Failed to list git notes")?;

    if !list_output.status.success() {
        return Ok(Vec::new());
    }

    let notes_list = String::from_utf8_lossy(&list_output.stdout);
    let commits: Vec<String> = notes_list
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 2 {
                Some(parts[1].to_string())
            } else {
                None
            }
        })
        .collect();

    Ok(commits)
}
