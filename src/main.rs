mod chunking;
mod db;
mod embed;
mod embeddings;
mod models;
mod semantic_branch;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::time::Instant;

struct IndexStats {
    indexed: usize,
    skipped: usize,
    resumed: usize,
    chunks: usize,
    deleted: usize,
}

#[derive(Parser)]
#[command(name = "gitsem")]
#[command(version, about = "Semantic search for your codebase")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Index all files and store embeddings on the semantic branch")]
    Index,

    #[command(about = "Hydrate local DB from the semantic branch")]
    Hydrate,

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

    #[command(about = "Inject agent instructions into CLAUDE.md")]
    AgenticSetup,

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
        Commands::Hydrate => {
            hydrate_from_branch()?;
        }
        Commands::Grep { query, max_count } => {
            grep_semantic(&query, max_count)?;
        }
        Commands::AgenticSetup => {
            agentic_setup()?;
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

fn make_progress_bar(total: usize) -> ProgressBar {
    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::with_template("{bar:40.green/black} {pos}/{len} {wide_msg}")
            .unwrap()
            .progress_chars("█▓░"),
    );
    pb
}

fn index_files_streaming(
    files: &[(PathBuf, String)],
    session: &semantic_branch::IndexSession,
    provider: &mut dyn embed::EmbeddingProvider,
) -> Result<IndexStats> {
    let pb = make_progress_bar(files.len());
    let mut stats = IndexStats {
        indexed: 0,
        skipped: 0,
        resumed: 0,
        chunks: 0,
        deleted: 0,
    };

    for (file_path, relative) in files {
        pb.set_message(relative.clone());

        if session.already_indexed(relative) {
            stats.resumed += 1;
            pb.inc(1);
            continue;
        }

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => {
                stats.skipped += 1;
                pb.inc(1);
                continue;
            }
        };

        let code_chunks = chunking::chunk_code(&content, Some(relative))?;
        let mut stored: Vec<semantic_branch::StoredChunk> = Vec::new();

        for code_chunk in code_chunks {
            let embedding = provider
                .generate_embedding(&code_chunk.text)
                .context("Failed to generate embedding")?;

            stored.push(semantic_branch::StoredChunk {
                start_line: code_chunk.start_line,
                end_line: code_chunk.end_line,
                text: code_chunk.text,
                embedding,
            });

            stats.chunks += 1;
        }

        session.write_file(relative, &stored)?;
        stats.indexed += 1;
        pb.inc(1);
    }

    pb.finish_and_clear();
    Ok(stats)
}

fn index_codebase() -> Result<()> {
    let repo_path = PathBuf::from(".");
    let started = Instant::now();

    let config = embed::EmbeddingConfig::load_or_default().unwrap_or_default();
    let mut provider = embed::create_provider(&config)?;
    provider.init()?;

    match semantic_branch::read_last_indexed_sha(&repo_path) {
        Some(last_sha) => {
            println!("Last indexed: {}", &last_sha[..8.min(last_sha.len())]);

            let changes = semantic_branch::get_changed_files(&repo_path, &last_sha)
                .context("Failed to compute changed files")?;

            if changes.is_empty() {
                println!("Already up to date.");
                return Ok(());
            }

            let to_embed: Vec<(PathBuf, String)> = changes
                .iter()
                .filter_map(|c| match c {
                    semantic_branch::FileChange::AddedOrModified(p) => {
                        Some((repo_path.join(p), p.clone()))
                    }
                    semantic_branch::FileChange::Renamed { to, .. } => {
                        Some((repo_path.join(to), to.clone()))
                    }
                    semantic_branch::FileChange::Deleted(_) => None,
                })
                .collect();

            let n_deleted = changes
                .iter()
                .filter(|c| matches!(c, semantic_branch::FileChange::Deleted(_)))
                .count();

            println!(
                "Changes since last index: {} to embed, {} to delete",
                to_embed.len(),
                n_deleted,
            );

            let session = semantic_branch::IndexSession::open(&repo_path, true)?;

            for change in &changes {
                if let semantic_branch::FileChange::Deleted(p)
                | semantic_branch::FileChange::Renamed { from: p, .. } = change
                {
                    session.delete_file(p)?;
                }
            }

            let stats = index_files_streaming(&to_embed, &session, provider.as_mut())?;

            session.commit()?;

            print_summary(&stats, started);
        }
        None => {
            let files = collect_files(&repo_path)?;
            let files: Vec<(PathBuf, String)> = files
                .into_iter()
                .map(|p| {
                    let rel = p
                        .strip_prefix(&repo_path)
                        .unwrap_or(&p)
                        .to_string_lossy()
                        .to_string();
                    (p, rel)
                })
                .collect();

            println!("Full index: {} tracked files", files.len());

            let session = semantic_branch::IndexSession::open(&repo_path, false)?;

            if session.has_partial_state() {
                println!("Resuming interrupted index...");
            }

            let stats = index_files_streaming(&files, &session, provider.as_mut())?;

            session.commit()?;

            print_summary(&stats, started);
        }
    }

    println!("Run `git-semantic hydrate` to populate the local search index.");
    println!("Run `git push origin semantic` to share with the team.");

    Ok(())
}

fn print_summary(stats: &IndexStats, started: Instant) {
    let elapsed = started.elapsed();
    let secs = elapsed.as_secs_f32();
    println!(
        "Done in {:.1}s — {} files indexed, {} chunks, {} skipped, {} deleted{}",
        secs,
        stats.indexed,
        stats.chunks,
        stats.skipped,
        stats.deleted,
        if stats.resumed > 0 {
            format!(", {} resumed", stats.resumed)
        } else {
            String::new()
        }
    );
}

fn hydrate_from_branch() -> Result<()> {
    let repo_path = PathBuf::from(".");

    println!("Reading chunks from semantic branch...");
    let file_chunks = semantic_branch::read_chunks_from_branch(&repo_path)
        .context("Failed to read from semantic branch")?;

    let total_files = file_chunks.len();
    let total_chunks: usize = file_chunks.iter().map(|(_, c)| c.len()).sum();

    println!("Found {} files, {} chunks total", total_files, total_chunks);

    let db = db::Database::init().context("Failed to initialize database")?;
    db.clear().context("Failed to clear existing index")?;

    for (file_path, chunks) in &file_chunks {
        for chunk in chunks {
            db.insert_chunk(&models::CodeChunk {
                file_path: file_path.clone(),
                start_line: chunk.start_line as i64,
                end_line: chunk.end_line as i64,
                content: chunk.text.clone(),
                embedding: chunk.embedding.clone(),
                distance: None,
            })
            .context("Failed to insert chunk")?;
        }
    }

    println!("Hydrated {} chunks into local index.", total_chunks);

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
        println!("No results found. Run `gitsem hydrate` first.");
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
        println!("  {}", chunk.content.lines().next().unwrap_or(""));
    }

    Ok(())
}

fn agentic_setup() -> Result<()> {
    const MARKER: &str = "<!-- gitsem:agentic-setup -->";
    const INSTRUCTIONS: &str = r#"<!-- gitsem:agentic-setup -->
## Code Search

Use `gitsem grep "<natural language query>"` instead of `git grep` or `grep` for all code searches.

Queries must describe intent, not syntax:
- BAD: `gitsem grep "fn authenticate"` — this is a keyword, not a query
- GOOD: `gitsem grep "user authentication and session handling"`
- BAD: `gitsem grep "impl Display"`
- GOOD: `gitsem grep "how types are formatted for display output"`

Use `-n` to control result count (default 10): `gitsem grep "query" -n 5`

Results show `[score] file:start-end` followed by a preview line. Lower score = more similar.
<!-- end gitsem:agentic-setup -->"#;

    let claude_md = PathBuf::from("CLAUDE.md");

    if claude_md.exists() {
        let existing = std::fs::read_to_string(&claude_md)?;
        if existing.contains(MARKER) {
            println!("CLAUDE.md already contains gitsem instructions — nothing to do.");
            return Ok(());
        }
        let mut file = std::fs::OpenOptions::new().append(true).open(&claude_md)?;
        use std::io::Write;
        write!(file, "\n\n{}", INSTRUCTIONS)?;
    } else {
        std::fs::write(&claude_md, INSTRUCTIONS)?;
    }

    println!("Injected gitsem instructions into CLAUDE.md.");
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
