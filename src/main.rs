mod chunking;
mod db;
mod embed;
mod embeddings;
mod models;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "gitsem")]
#[command(version, about = "Semantic search for your codebase")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Index all files in the repository")]
    Index,

    #[command(about = "Search code semantically using natural language")]
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

    #[command(about = "Get and set gitsem options")]
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
        Commands::Index => {
            index_codebase()?;
        }
        Commands::Grep { query, max_count } => {
            grep_semantic(&query, max_count)?;
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

fn collect_files(repo_path: &PathBuf) -> Result<Vec<PathBuf>> {
    let output = std::process::Command::new("git")
        .current_dir(repo_path)
        .args(["ls-files"])
        .output()
        .context("Failed to run git ls-files")?;

    if !output.status.success() {
        anyhow::bail!("git ls-files failed");
    }

    let files = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| repo_path.join(line))
        .collect();

    Ok(files)
}

fn index_codebase() -> Result<()> {
    let repo_path = PathBuf::from(".");

    let files = collect_files(&repo_path)?;
    println!("Found {} tracked files", files.len());

    let db = db::Database::init().context("Failed to initialize database")?;
    db.clear().context("Failed to clear existing index")?;

    let mut total_chunks = 0;
    let mut skipped = 0;

    for file_path in &files {
        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };

        let relative = file_path
            .strip_prefix(&repo_path)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        let code_chunks = chunking::chunk_code(&content, Some(&relative))?;

        for code_chunk in code_chunks {
            let vector_embedding = embed::generate_embedding(&code_chunk.text)
                .context("Failed to generate embedding")?;

            db.insert_chunk(&models::CodeChunk {
                file_path: relative.clone(),
                start_line: code_chunk.start_line as i64,
                end_line: code_chunk.end_line as i64,
                content: code_chunk.text,
                embedding: vector_embedding,
                distance: None,
            })
            .context("Failed to insert chunk")?;

            total_chunks += 1;
        }
    }

    println!(
        "Indexed {} chunks from {} files ({} skipped)",
        total_chunks,
        files.len() - skipped,
        skipped
    );

    Ok(())
}

fn grep_semantic(query: &str, max_count: i64) -> Result<()> {
    let db = db::Database::init().context("Failed to initialize database")?;

    let query_embedding =
        embed::generate_embedding(query).context("Failed to generate query embedding")?;

    let results = db
        .search_similar(&query_embedding, max_count)
        .context("Failed to search database")?;

    if results.is_empty() {
        println!("No results found. Run `gitsem index` first.");
        return Ok(());
    }

    for chunk in results.iter() {
        let dist = chunk
            .distance
            .map(|d| format!("{:.3}", d))
            .unwrap_or_else(|| "N/A".to_string());

        println!(
            "[{}] {}:{}-{}",
            dist, chunk.file_path, chunk.start_line, chunk.end_line
        );
        println!(
            "  {}",
            chunk.content.lines().next().unwrap_or("")
        );
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
