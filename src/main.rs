mod models;
mod db;
mod git;
mod embed;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "git-semantic")]
#[command(about = "Semantic search layer for Git repositories", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Pull {
        #[arg(help = "Remote name (default: origin)")]
        remote: Option<String>,
    },
    Commit {
        #[arg(short, long, help = "Commit message")]
        message: Option<String>,
        #[arg(short, long, help = "Stage all changes before committing")]
        all: bool,
    },
    Grep {
        #[arg(help = "Search query")]
        query: String,
        #[arg(short = 'n', long, default_value = "10", help = "Maximum number of results")]
        max_count: i64,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Pull { remote } => {
            pull_and_sync(remote.as_deref())?;
        }
        Commands::Commit { message, all } => {
            commit_with_notes(message.as_deref(), all)?;
        }
        Commands::Grep { query, max_count } => {
            grep_semantic(&query, max_count)?;
        }
    }

    Ok(())
}

fn pull_and_sync(remote: Option<&str>) -> Result<()> {
    let remote_name = remote.unwrap_or("origin");
    let repo_path = PathBuf::from(".");

    let status_output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .context("Failed to check git status")?;

    let is_clean = status_output.stdout.is_empty();

    if !is_clean {
        let diff_output = std::process::Command::new("git")
            .args(["diff", "--quiet"])
            .status();

        match diff_output {
            Ok(status) if !status.success() => {
                println!("Working directory has uncommitted changes, pulling anyway...");
            }
            _ => {}
        }
    }

    let branch_output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .context("Failed to get current branch")?;

    let current_branch = String::from_utf8_lossy(&branch_output.stdout).trim().to_string();

    let upstream_check = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", &format!("{}@{{upstream}}", current_branch)])
        .output();

    let needs_pull = match upstream_check {
        Ok(output) if output.status.success() => {
            let local_commit = std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .output()?;

            let remote_commit = std::process::Command::new("git")
                .args(["rev-parse", &format!("{}@{{upstream}}", current_branch)])
                .output()?;

            local_commit.stdout != remote_commit.stdout
        }
        _ => {
            println!("No upstream branch configured, skipping pull");
            false
        }
    };

    if needs_pull {
        println!("Pulling from {}...", remote_name);
        std::process::Command::new("git")
            .arg("pull")
            .arg(remote_name)
            .arg(&current_branch)
            .status()
            .context("Failed to execute git pull")?;
    } else {
        println!("Already up to date with {}", remote_name);
    }

    println!("Syncing semantic notes from refs/notes/semantic...");

    let fetch_result = std::process::Command::new("git")
        .args(["fetch", remote_name, "refs/notes/semantic:refs/notes/semantic"])
        .status();

    match fetch_result {
        Ok(_) => println!("Semantic notes fetched"),
        Err(_) => println!("No semantic notes found on remote, skipping..."),
    }

    println!("Rebuilding local semantic index...");

    let db = db::Database::init()
        .context("Failed to initialize database")?;

    let notes = git::read_notes(&repo_path)
        .context("Failed to read semantic notes")?;

    if notes.is_empty() {
        println!("No semantic notes to sync");
        return Ok(());
    }

    let mut total_chunks = 0;
    for (commit_sha, chunks) in notes {
        println!("Processing {} chunks from commit {}...", chunks.len(), &commit_sha[..8]);
        for chunk in chunks {
            db.insert_chunk(&chunk)
                .context("Failed to insert chunk into database")?;
            total_chunks += 1;
        }
    }

    println!("Semantic index synchronized: {} chunks from {} commits",
             total_chunks,
             git::list_commits_with_notes(&repo_path)?.len());

    Ok(())
}

fn commit_with_notes(message: Option<&str>, all: bool) -> Result<()> {
    let repo_path = PathBuf::from(".");

    let status_output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .context("Failed to check git status")?;

    let has_changes = !status_output.stdout.is_empty();

    if all && has_changes {
        println!("Staging all changes...");
        std::process::Command::new("git")
            .arg("add")
            .arg("-A")
            .status()
            .context("Failed to stage changes")?;
    }

    let staged_output = std::process::Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .status();

    let has_staged = match staged_output {
        Ok(status) => !status.success(),
        Err(_) => false,
    };

    let repo = gix::open(&repo_path)
        .context("Failed to open Git repository")?;

    let head_before = repo.head()
        .context("Failed to get HEAD reference")?;

    let commit_id_before = head_before.into_peeled_id()
        .context("Failed to resolve HEAD to commit")?;

    let commit_sha_before = commit_id_before.to_string();

    let (commit_sha, commit_message, diff_text) = if has_staged {
        let commit_message = if let Some(msg) = message {
            msg.to_string()
        } else {
            println!("Enter commit message:");
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            input.trim().to_string()
        };

        println!("Analyzing staged changes...");

        let diff_output = std::process::Command::new("git")
            .args(["diff", "--cached"])
            .output()
            .context("Failed to get staged diff")?;

        let diff_text = String::from_utf8_lossy(&diff_output.stdout).to_string();

        println!("Creating commit...");
        let commit_result = std::process::Command::new("git")
            .arg("commit")
            .arg("-m")
            .arg(&commit_message)
            .status()
            .context("Failed to create commit")?;

        if !commit_result.success() {
            anyhow::bail!("Git commit failed");
        }

        let repo_refreshed = gix::open(&repo_path)
            .context("Failed to reopen repository")?;

        let head_after = repo_refreshed.head()
            .context("Failed to get HEAD reference after commit")?;

        let commit_id_after = head_after.into_peeled_id()
            .context("Failed to resolve HEAD to commit")?;

        let commit_sha_after = commit_id_after.to_string();

        (commit_sha_after, commit_message, diff_text)
    } else {
        println!("No staged changes, checking if HEAD needs semantic notes...");

        let last_commit_output = std::process::Command::new("git")
            .args(["log", "-1", "--format=%B"])
            .output()
            .context("Failed to get last commit message")?;

        let last_commit_message = String::from_utf8_lossy(&last_commit_output.stdout)
            .trim()
            .to_string();

        let diff_output = std::process::Command::new("git")
            .args(["show", "--format=", "HEAD"])
            .output()
            .context("Failed to get last commit diff")?;

        let diff_text = String::from_utf8_lossy(&diff_output.stdout).to_string();

        (commit_sha_before, last_commit_message, diff_text)
    };

    println!("Generating semantic embeddings for commit {}...", &commit_sha[..8]);

    let textual_note = format!(
        "Commit: {}\nMessage: {}\n\nDiff:\n{}",
        commit_sha, commit_message, diff_text
    );

    let vector_embedding = embed::generate_embedding(&diff_text)
        .context("Failed to generate embedding for diff")?;

    git::write_note(
        &repo_path,
        &commit_sha,
        &[models::CodeChunk {
            file_path: "commit".to_string(),
            start_line: 0,
            end_line: 0,
            content: textual_note,
            commit_sha: commit_sha.clone(),
            embedding: vector_embedding,
        }],
    )
    .context("Failed to write semantic notes")?;

    println!("✓ Semantic notes attached to commit {}", &commit_sha[..8]);
    println!("  - Note stored in refs/notes/semantic");
    println!("  - Contains textual context and vector embeddings");
    println!("\nTo share with team:");
    println!("  git push origin refs/notes/semantic");

    Ok(())
}

fn grep_semantic(query: &str, max_count: i64) -> Result<()> {
    let db = db::Database::init()
        .context("Failed to initialize database")?;

    let query_embedding = embed::generate_embedding(query)
        .context("Failed to generate query embedding")?;

    let results = db.search_similar(&query_embedding, max_count)
        .context("Failed to search database")?;

    for chunk in results.iter() {
        println!("{}:{}:{}", chunk.file_path, chunk.start_line, chunk.content.lines().next().unwrap_or(""));
    }

    Ok(())
}
