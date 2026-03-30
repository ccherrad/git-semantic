use crate::models::CodeChunk;
use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

const SEMANTIC_NOTES_REF: &str = "refs/notes/semantic";

pub fn write_note(repo_path: &Path, commit_sha: &str, chunks: &[CodeChunk]) -> Result<()> {
    let note_content =
        serde_json::to_string_pretty(&chunks).context("Failed to serialize chunks to JSON")?;

    let temp_file = repo_path.join(".git").join("semantic-note-temp.json");
    std::fs::write(&temp_file, &note_content).context("Failed to write temporary note file")?;

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
        .args(["notes", "--ref", SEMANTIC_NOTES_REF, "list"])
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
            .args(["notes", "--ref", SEMANTIC_NOTES_REF, "show", commit_sha])
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
                eprintln!(
                    "Warning: Failed to parse note for commit {}: {}",
                    commit_sha, e
                );
                continue;
            }
        }
    }

    Ok(all_notes)
}

pub fn list_commits_with_notes(repo_path: &Path) -> Result<Vec<String>> {
    let list_output = Command::new("git")
        .current_dir(repo_path)
        .args(["notes", "--ref", SEMANTIC_NOTES_REF, "list"])
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::CodeChunk;

    #[test]
    fn test_semantic_notes_ref_constant() {
        assert_eq!(SEMANTIC_NOTES_REF, "refs/notes/semantic");
    }

    #[test]
    fn test_write_and_read_notes() {
        let temp_dir = std::env::temp_dir().join(format!("test_git_{}", std::process::id()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        std::process::Command::new("git")
            .current_dir(&temp_dir)
            .args(["init"])
            .output()
            .unwrap();

        std::process::Command::new("git")
            .current_dir(&temp_dir)
            .args(["config", "user.name", "Test User"])
            .output()
            .unwrap();

        std::process::Command::new("git")
            .current_dir(&temp_dir)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();

        let test_file = temp_dir.join("test.txt");
        std::fs::write(&test_file, "test content").unwrap();

        std::process::Command::new("git")
            .current_dir(&temp_dir)
            .args(["add", "test.txt"])
            .output()
            .unwrap();

        std::process::Command::new("git")
            .current_dir(&temp_dir)
            .args(["commit", "-m", "test commit"])
            .output()
            .unwrap();

        let commit_sha_output = std::process::Command::new("git")
            .current_dir(&temp_dir)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();

        let commit_sha = String::from_utf8_lossy(&commit_sha_output.stdout)
            .trim()
            .to_string();

        let chunk = CodeChunk {
            file_path: "test.txt".to_string(),
            start_line: 0,
            end_line: 1,
            content: "test content".to_string(),
            commit_sha: commit_sha.clone(),
            embedding: vec![0.1, 0.2, 0.3],
            distance: None,
        };

        let write_result = write_note(&temp_dir, &commit_sha, &[chunk.clone()]);
        assert!(write_result.is_ok());

        let notes = read_notes(&temp_dir).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].0, commit_sha);
        assert_eq!(notes[0].1[0].file_path, "test.txt");

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_list_commits_with_notes() {
        let temp_dir = std::env::temp_dir().join(format!("test_git_list_{}", std::process::id()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        std::process::Command::new("git")
            .current_dir(&temp_dir)
            .args(["init"])
            .output()
            .unwrap();

        let commits = list_commits_with_notes(&temp_dir).unwrap();
        assert_eq!(commits.len(), 0);

        std::fs::remove_dir_all(&temp_dir).ok();
    }
}
