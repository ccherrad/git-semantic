mod db;
mod embed;
mod embeddings;
mod git;
mod models;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "gitsem")]
#[command(version, about = "Semantic search layer for Git repositories")]
#[command(
    long_about = "gitsem augments Git commits with vector embeddings, enabling semantic code search.\n\n\
Features:\n\
  • Attach semantic notes (embeddings + context) to commits\n\
  • Search code by meaning using natural language queries\n\
  • Share semantic indexes with team via Git notes\n\
  • Retroactively index existing commit history\n\n\
Storage:\n\
  • Git notes stored in refs/notes/semantic\n\
  • Local SQLite index at .git/semantic.db\n\n\
Examples:\n\
  gitsem commit -a -m \"Add authentication\"\n\
  gitsem reindex HEAD~10..HEAD\n\
  gitsem grep \"error handling logic\"\n\
  gitsem pull"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Pull changes and sync semantic notes from remote")]
    #[command(
        long_about = "Performs git pull and fetches refs/notes/semantic from remote.\n\n\
Rebuilds local SQLite database from all semantic notes.\n\n\
Example:\n\
  gitsem pull\n\
  gitsem pull upstream"
    )]
    Pull {
        #[arg(help = "Remote name (default: origin)")]
        remote: Option<String>,
    },

    #[command(about = "Create commit with semantic notes attached")]
    #[command(
        long_about = "Creates a Git commit and attaches semantic notes containing:\n\
  • Commit message and metadata\n\
  • Full diff of changes\n\
  • Vector embeddings (768-dim)\n\n\
The note is stored in refs/notes/semantic and can be shared with:\n\
  git push origin refs/notes/semantic\n\n\
Examples:\n\
  gitsem commit -a -m \"Add user login\"\n\
  gitsem commit -m \"Fix bug in parser\"\n\
  gitsem commit  (interactive mode)"
    )]
    Commit {
        #[arg(short, long, help = "Commit message")]
        message: Option<String>,
        #[arg(short, long, help = "Stage all changes before committing")]
        all: bool,
    },

    #[command(about = "Search code semantically using natural language")]
    #[command(long_about = "Performs vector similarity search on indexed code.\n\n\
Generates embedding for query and finds semantically similar code chunks\n\
using KNN search in the local SQLite database.\n\n\
Note: Database must be populated first via 'gitsem pull'\n\n\
Examples:\n\
  gitsem grep \"authentication logic\"\n\
  gitsem grep \"error handling\" -n 5")]
    Grep {
        #[arg(help = "Search query in natural language")]
        query: String,
        #[arg(
            short = 'n',
            long,
            default_value = "10",
            help = "Maximum number of results"
        )]
        max_count: i64,
    },

    #[command(about = "Add semantic notes to existing commits")]
    #[command(
        long_about = "Retroactively adds semantic notes to commits in the specified range.\n\n\
Useful for indexing existing repositories or adding notes to commits\n\
created with regular 'git commit'.\n\n\
For each commit:\n\
  • Extracts commit message and diff\n\
  • Generates vector embeddings\n\
  • Attaches note to refs/notes/semantic\n\n\
Examples:\n\
  gitsem reindex HEAD~3..HEAD    (last 3 commits)\n\
  gitsem reindex main..HEAD      (all commits since main)\n\
  gitsem reindex abc123..def456  (specific range)"
    )]
    Reindex {
        #[arg(help = "Commit range (e.g., HEAD~3, main..HEAD, abc123..def456)")]
        range: String,
    },

    #[command(about = "Display semantic note for a commit")]
    #[command(
        long_about = "Shows the semantic note attached to a commit with formatted display.\n\n\
Displays:\n\
  • Commit SHA and metadata\n\
  • Embedding dimensions\n\
  • Content preview (diff and context)\n\n\
Examples:\n\
  gitsem show           (show HEAD)\n\
  gitsem show abc123    (specific commit)\n\
  gitsem show HEAD~2    (2 commits back)"
    )]
    Show {
        #[arg(
            help = "Commit SHA or reference (default: HEAD)",
            default_value = "HEAD"
        )]
        commit: String,
    },

    #[command(about = "Get and set gitsem options")]
    #[command(long_about = "Configure gitsem settings using git config.\n\n\
Settings are stored in .git/config under the [gitsem] section.\n\n\
Examples:\n\
  gitsem config --list\n\
  gitsem config gitsem.provider onnx\n\
  gitsem config gitsem.openai.model text-embedding-3-large\n\
  gitsem config --get gitsem.provider\n\
  gitsem config --unset gitsem.onnx.modelPath")]
    Config {
        #[arg(help = "Configuration key (e.g., gitsem.provider)")]
        key: Option<String>,

        #[arg(help = "Configuration value")]
        value: Option<String>,

        #[arg(long, help = "List all gitsem configuration")]
        list: bool,

        #[arg(long, help = "Get the value for a given key")]
        get: bool,

        #[arg(long, help = "Remove a configuration key")]
        unset: bool,
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
        Commands::Reindex { range } => {
            reindex_commits(&range)?;
        }
        Commands::Show { commit } => {
            show_semantic_note(&commit)?;
        }
        Commands::Config {
            key,
            value,
            list,
            get,
            unset,
        } => {
            config_command(key.as_deref(), value.as_deref(), list, get, unset)?;
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

    let current_branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();

    let upstream_check = std::process::Command::new("git")
        .args([
            "rev-parse",
            "--abbrev-ref",
            &format!("{}@{{upstream}}", current_branch),
        ])
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
        .args([
            "fetch",
            remote_name,
            "refs/notes/semantic:refs/notes/semantic",
        ])
        .status();

    match fetch_result {
        Ok(_) => println!("Semantic notes fetched"),
        Err(_) => println!("No semantic notes found on remote, skipping..."),
    }

    println!("Rebuilding local semantic index...");

    let db = db::Database::init().context("Failed to initialize database")?;

    let notes = git::read_notes(&repo_path).context("Failed to read semantic notes")?;

    if notes.is_empty() {
        println!("No semantic notes to sync");
        return Ok(());
    }

    let mut total_chunks = 0;
    for (commit_sha, chunks) in notes {
        println!(
            "Processing {} chunks from commit {}...",
            chunks.len(),
            &commit_sha[..8]
        );
        for chunk in chunks {
            db.insert_chunk(&chunk)
                .context("Failed to insert chunk into database")?;
            total_chunks += 1;
        }
    }

    println!(
        "Semantic index synchronized: {} chunks from {} commits",
        total_chunks,
        git::list_commits_with_notes(&repo_path)?.len()
    );

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

    let repo = gix::open(&repo_path).context("Failed to open Git repository")?;

    let head_before = repo.head().context("Failed to get HEAD reference")?;

    let commit_id_before = head_before
        .into_peeled_id()
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

        let repo_refreshed = gix::open(&repo_path).context("Failed to reopen repository")?;

        let head_after = repo_refreshed
            .head()
            .context("Failed to get HEAD reference after commit")?;

        let commit_id_after = head_after
            .into_peeled_id()
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

    println!(
        "Generating semantic embeddings for commit {}...",
        &commit_sha[..8]
    );

    let textual_note = format!(
        "Commit: {}\nMessage: {}\n\nDiff:\n{}",
        commit_sha, commit_message, diff_text
    );

    let vector_embedding =
        embed::generate_embedding(&diff_text).context("Failed to generate embedding for diff")?;

    let chunk = models::CodeChunk {
        file_path: "commit".to_string(),
        start_line: 0,
        end_line: 0,
        content: textual_note,
        commit_sha: commit_sha.clone(),
        embedding: vector_embedding,
        distance: None,
    };

    git::write_note(&repo_path, &commit_sha, std::slice::from_ref(&chunk))
        .context("Failed to write semantic notes")?;

    let db = db::Database::init().context("Failed to initialize database")?;

    db.insert_chunk(&chunk)
        .context("Failed to insert chunk into database")?;

    println!("✓ Semantic notes attached to commit {}", &commit_sha[..8]);
    println!("  - Note stored in refs/notes/semantic");
    println!("  - Contains textual context and vector embeddings");
    println!("  - Database indexed for semantic search");
    println!("\nTo share with team:");
    println!("  git push origin refs/notes/semantic");

    Ok(())
}

fn grep_semantic(query: &str, max_count: i64) -> Result<()> {
    let db = db::Database::init().context("Failed to initialize database")?;

    let query_embedding =
        embed::generate_embedding(query).context("Failed to generate query embedding")?;

    let results = db
        .search_similar(&query_embedding, max_count)
        .context("Failed to search database")?;

    for chunk in results.iter() {
        let similarity = if let Some(dist) = chunk.distance {
            format!("dist={:.3}", dist)
        } else {
            "N/A".to_string()
        };

        println!(
            "[{}] {}:{}:{}",
            similarity,
            chunk.file_path,
            chunk.start_line,
            chunk.content.lines().next().unwrap_or("")
        );
    }

    Ok(())
}

fn reindex_commits(range: &str) -> Result<()> {
    let repo_path = PathBuf::from(".");

    println!("Fetching commits in range: {}", range);

    let log_output = std::process::Command::new("git")
        .current_dir(&repo_path)
        .args(["log", "--format=%H", range])
        .output()
        .context("Failed to get commit list")?;

    if !log_output.status.success() {
        let stderr = String::from_utf8_lossy(&log_output.stderr);
        anyhow::bail!("Failed to parse commit range: {}", stderr);
    }

    let commit_list = String::from_utf8_lossy(&log_output.stdout);
    let commits: Vec<&str> = commit_list.lines().collect();

    if commits.is_empty() {
        println!("No commits found in range: {}", range);
        return Ok(());
    }

    println!("Found {} commits to reindex", commits.len());

    let db = db::Database::init().context("Failed to initialize database")?;

    let mut all_chunks = Vec::new();

    for (i, commit_sha) in commits.iter().enumerate() {
        println!(
            "\n[{}/{}] Processing commit {}...",
            i + 1,
            commits.len(),
            &commit_sha[..8]
        );

        let commit_msg_output = std::process::Command::new("git")
            .current_dir(&repo_path)
            .args(["log", "-1", "--format=%B", commit_sha])
            .output()
            .context("Failed to get commit message")?;

        let commit_message = String::from_utf8_lossy(&commit_msg_output.stdout)
            .trim()
            .to_string();

        let diff_output = std::process::Command::new("git")
            .current_dir(&repo_path)
            .args(["show", "--format=", commit_sha])
            .output()
            .context("Failed to get commit diff")?;

        let diff_text = String::from_utf8_lossy(&diff_output.stdout).to_string();

        if diff_text.is_empty() {
            println!("  ⊘ Skipping (no changes)");
            continue;
        }

        println!("  Generating embeddings...");
        let vector_embedding = embed::generate_embedding(&diff_text)
            .context("Failed to generate embedding for diff")?;

        let textual_note = format!(
            "Commit: {}\nMessage: {}\n\nDiff:\n{}",
            commit_sha, commit_message, diff_text
        );

        let chunk = models::CodeChunk {
            file_path: "commit".to_string(),
            start_line: 0,
            end_line: 0,
            content: textual_note,
            commit_sha: commit_sha.to_string(),
            embedding: vector_embedding,
            distance: None,
        };

        git::write_note(&repo_path, commit_sha, std::slice::from_ref(&chunk))
            .context("Failed to write semantic note")?;

        db.insert_chunk(&chunk)
            .context("Failed to insert chunk into database")?;

        all_chunks.push(chunk);

        println!("  ✓ Semantic note attached and indexed");
    }

    println!(
        "\n✓ Reindexing complete: {} commits processed",
        commits.len()
    );
    println!("✓ Database hydrated with {} chunks", all_chunks.len());
    println!("\nTo share with team:");
    println!("  git push origin refs/notes/semantic");

    Ok(())
}

fn show_semantic_note(commit: &str) -> Result<()> {
    let repo_path = PathBuf::from(".");

    let resolve_output = std::process::Command::new("git")
        .current_dir(&repo_path)
        .args(["rev-parse", commit])
        .output()
        .context("Failed to resolve commit")?;

    if !resolve_output.status.success() {
        anyhow::bail!("Invalid commit: {}", commit);
    }

    let commit_sha = String::from_utf8_lossy(&resolve_output.stdout)
        .trim()
        .to_string();

    let show_output = std::process::Command::new("git")
        .current_dir(&repo_path)
        .args(["notes", "--ref", "refs/notes/semantic", "show", &commit_sha])
        .output()
        .context("Failed to show semantic note")?;

    if !show_output.status.success() {
        println!("No semantic note found for commit {}", &commit_sha[..8]);
        println!("\nTo add semantic notes:");
        println!("  gitsem commit              (for new commits)");
        println!("  gitsem reindex HEAD~3..HEAD (for existing commits)");
        return Ok(());
    }

    let note_content = String::from_utf8_lossy(&show_output.stdout);

    match serde_json::from_str::<Vec<models::CodeChunk>>(&note_content) {
        Ok(chunks) => {
            println!("Semantic note for commit {}\n", &commit_sha[..8]);

            for (i, chunk) in chunks.iter().enumerate() {
                println!("─── Chunk {} ───", i + 1);
                println!("File:       {}", chunk.file_path);
                println!("Lines:      {}-{}", chunk.start_line, chunk.end_line);
                println!("Commit:     {}", &chunk.commit_sha[..8]);
                println!("Embedding:  {} dimensions", chunk.embedding.len());
                println!("\nContent Preview:");
                let preview: Vec<&str> = chunk.content.lines().take(10).collect();
                for line in preview {
                    println!("  {}", line);
                }
                if chunk.content.lines().count() > 10 {
                    println!("  ... ({} more lines)", chunk.content.lines().count() - 10);
                }
                println!();
            }
        }
        Err(e) => {
            println!("Raw semantic note for commit {}\n", &commit_sha[..8]);
            println!("(Failed to parse as structured data: {})\n", e);
            println!("{}", note_content);
        }
    }

    Ok(())
}

fn config_command(
    key: Option<&str>,
    value: Option<&str>,
    list: bool,
    get: bool,
    unset: bool,
) -> Result<()> {
    use embed::EmbeddingConfig;

    if list {
        EmbeddingConfig::show()?;
        return Ok(());
    }

    if unset {
        let key = key.context("Key required for --unset")?;
        EmbeddingConfig::unset_git_config(key)?;
        println!("Unset {}", key);
        return Ok(());
    }

    if get {
        let key = key.context("Key required for --get")?;
        if let Some(value) = EmbeddingConfig::get_git_config(key) {
            println!("{}", value);
        } else {
            anyhow::bail!("Configuration key '{}' not found", key);
        }
        return Ok(());
    }

    if let (Some(key), Some(value)) = (key, value) {
        EmbeddingConfig::set_git_config(key, value)?;
        println!("Set {} = {}", key, value);
        return Ok(());
    }

    if let Some(key) = key {
        if let Some(value) = EmbeddingConfig::get_git_config(key) {
            println!("{}", value);
        } else {
            anyhow::bail!("Configuration key '{}' not found", key);
        }
        return Ok(());
    }

    EmbeddingConfig::show()?;
    Ok(())
}
