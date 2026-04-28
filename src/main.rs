mod chunking;
mod clustering;
mod db;
mod embed;
mod embeddings;
mod map;
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
#[command(name = "semantic")]
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

    #[command(about = "Enable a coding agent for this project (e.g. claude)")]
    Enable {
        #[arg(help = "Agent to enable: claude")]
        agent: String,
    },

    #[command(about = "Show token usage for the current project's Claude Code sessions")]
    Usage {
        #[arg(short, long, help = "Number of sessions to show", default_value = "5")]
        sessions: usize,
        #[arg(
            short,
            long,
            help = "Watch mode: refresh every N seconds",
            value_name = "SECS"
        )]
        watch: Option<u64>,
    },

    #[command(about = "Show the codebase map or find subsystems matching a query")]
    Map {
        #[arg(help = "Natural language query to find matching subsystems (optional)")]
        query: Option<String>,
    },

    #[command(about = "Retrieve a specific chunk by file and line range")]
    Get {
        #[arg(help = "Chunk location in format file:start-end (e.g. src/db.rs:12-34)")]
        location: String,
    },

    #[command(about = "Get and set semantic options")]
    Config {
        #[arg(help = "Configuration key (e.g., semantic.provider)")]
        key: Option<String>,

        #[arg(help = "Configuration value")]
        value: Option<String>,

        #[arg(long, help = "List all semantic configuration")]
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
        Commands::Map { query } => {
            map_command(query.as_deref())?;
        }
        Commands::Get { location } => {
            get_command(&location)?;
        }
        Commands::Enable { agent } => match agent.as_str() {
            "claude" => claude_setup()?,
            other => anyhow::bail!("Unknown agent '{}'. Supported: claude", other),
        },
        Commands::Usage { sessions, watch } => {
            if let Some(interval) = watch {
                let secs = if interval == 0 { 2 } else { interval };
                loop {
                    print!("\x1B[2J\x1B[H");
                    show_usage(sessions)?;
                    println!("\nRefreshing every {}s — Ctrl+C to stop", secs);
                    std::thread::sleep(std::time::Duration::from_secs(secs));
                }
            } else {
                show_usage(sessions)?;
            }
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

            println!("Building semantic map...");
            let all_chunks =
                semantic_branch::read_all_chunks_from_worktree(session.worktree_path())?;
            let cluster_inputs: Vec<clustering::ClusterInput> = all_chunks
                .into_iter()
                .flat_map(|(file, chunks)| {
                    chunks
                        .into_iter()
                        .map(move |chunk| clustering::ClusterInput {
                            file: file.clone(),
                            chunk,
                        })
                })
                .collect();
            let map = clustering::build_map(&cluster_inputs, &mut |text| {
                provider.generate_embedding(text)
            })?;

            session.commit(&map)?;

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

            println!("Building semantic map...");
            let all_chunks =
                semantic_branch::read_all_chunks_from_worktree(session.worktree_path())?;
            let cluster_inputs: Vec<clustering::ClusterInput> = all_chunks
                .into_iter()
                .flat_map(|(file, chunks)| {
                    chunks
                        .into_iter()
                        .map(move |chunk| clustering::ClusterInput {
                            file: file.clone(),
                            chunk,
                        })
                })
                .collect();
            let map = clustering::build_map(&cluster_inputs, &mut |text| {
                provider.generate_embedding(text)
            })?;

            session.commit(&map)?;

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

    match semantic_branch::read_semantic_map_from_branch(&repo_path) {
        Ok(map) => {
            for subsystem in &map.subsystems {
                db.insert_subsystem(subsystem)
                    .context("Failed to insert subsystem")?;
            }
            for edge in &map.edges {
                db.insert_edge(edge).context("Failed to insert edge")?;
            }
            println!(
                "Loaded map: {} subsystems, {} edges.",
                map.subsystems.len(),
                map.edges.len()
            );
        }
        Err(_) => {
            println!("No semantic map on branch yet (run `git-semantic index` to build one).");
        }
    }

    Ok(())
}

fn grep_semantic(query: &str, max_count: i64) -> Result<()> {
    let db = db::Database::init().context("Failed to initialize database")?;

    let query_embedding =
        embed::generate_embedding(query).context("Failed to generate query embedding")?;

    let results = db
        .search_hybrid(query, &query_embedding, max_count)
        .context("Failed to search database")?;

    if results.is_empty() {
        println!("No results found. Run `semantic hydrate` first.");
        return Ok(());
    }

    for chunk in results.iter() {
        let score = chunk
            .distance
            .map(|d| format!("{:.4}", d))
            .unwrap_or_else(|| "N/A".to_string());

        println!(
            "[{}] {}:{}-{}",
            score, chunk.file_path, chunk.start_line, chunk.end_line
        );
        println!("{}", chunk.content);
        println!("---");
    }

    Ok(())
}

fn claude_setup() -> Result<()> {
    let agents_dir = PathBuf::from(".claude/agents");
    std::fs::create_dir_all(&agents_dir).context("Failed to create .claude/agents")?;

    let agent_path = agents_dir.join("git-semantic.md");

    if agent_path.exists() {
        println!(".claude/agents/git-semantic.md already exists — nothing to do.");
        return Ok(());
    }

    let agent_content = r#"---
name: git-semantic
description: Use this agent when searching, navigating, or understanding code in this repository. Invoke it when you need to find where something is implemented, understand how a subsystem works, locate a function or type, or explore code before making changes.
---

You are a code navigation agent. Use the three git-semantic commands below to orient and retrieve code. Do not load whole files or use grep.

## Workflow

**Step 1 — Orient**
```bash
git-semantic map "<natural language query>"
```
Read the output carefully. If it names the function or type you need, go directly to step 2.

**Step 2 — Retrieve**
```bash
git-semantic get <file:start-end>
```
Use locations from the map output directly. Max 3 calls per task.

**Step 3 — Search (last resort)**
```bash
git-semantic grep "<natural language query>"
```
Only if the map did not surface what you need. Lower score = more similar.

## Rules

- Never re-fetch a chunk already in context.
- The map output IS the answer — do not re-search what the map already named.
- If the map description contains the function/type name you need, use `get` immediately.
- Max 3 `get` calls per task. If you need more, you are over-reading.
"#;

    std::fs::write(&agent_path, agent_content)?;
    println!("wrote .claude/agents/git-semantic.md");
    println!("\nCall with @git-semantic when you need to navigate or search code.");
    Ok(())
}

fn waste_bar(turns: &[u64], baseline: f64) -> String {
    let blocks = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
    let max = *turns.iter().max().unwrap_or(&1) as f64;
    turns
        .iter()
        .enumerate()
        .map(|(i, &t)| {
            let height = ((t as f64 / max) * 7.0).round() as usize;
            let bar = blocks[height.min(7)];
            if i < 5 {
                bar.to_string()
            } else {
                let ratio = t as f64 / baseline;
                if ratio >= 5.0 {
                    format!("\x1B[31m{}\x1B[0m", bar)
                } else if ratio >= 2.5 {
                    format!("\x1B[33m{}\x1B[0m", bar)
                } else {
                    bar.to_string()
                }
            }
        })
        .collect()
}

fn show_usage(max_sessions: usize) -> Result<()> {
    let projects_dir = dirs_home()?.join(".claude").join("projects");
    if !projects_dir.exists() {
        println!(
            "No Claude Code sessions found at {}",
            projects_dir.display()
        );
        return Ok(());
    }

    let cwd = std::env::current_dir()?;
    let cwd_key = cwd.to_string_lossy().replace('/', "-");

    let project_dir = std::fs::read_dir(&projects_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .contains(cwd_key.trim_start_matches('-'))
        });

    let project_dir = match project_dir {
        Some(d) => d.path(),
        None => {
            println!("No sessions found for current project ({}).", cwd.display());
            return Ok(());
        }
    };

    let mut all_sessions: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();

    for session_entry in std::fs::read_dir(&project_dir)? {
        let session_entry = session_entry?;
        let path = session_entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            let mtime = session_entry.metadata()?.modified()?;
            all_sessions.push((mtime, path));
        }
    }

    all_sessions.sort_by(|a, b| b.0.cmp(&a.0));

    if all_sessions.is_empty() {
        println!("No sessions found.");
        return Ok(());
    }

    println!("Project: {}", cwd.display());
    println!();
    println!(
        "{:<14} {:<8} {:<12} {:<12} {:<10} {:<10} GROWTH",
        "SESSION", "TURNS", "BASELINE", "LATEST", "WASTE", "TOTAL"
    );
    println!("{}", "-".repeat(80));

    let mut first = true;
    let mut hottest: Option<(usize, u64, f64)> = None;

    for (_, path) in all_sessions.iter().take(max_sessions) {
        let session_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        let turns = parse_session_turns(path.to_path_buf());
        if turns.is_empty() {
            continue;
        }

        let baseline = turns[..5.min(turns.len())].iter().sum::<u64>() as f64
            / 5.0_f64.min(turns.len() as f64);
        let latest = turns[turns.len().saturating_sub(3)..].iter().sum::<u64>() as f64
            / 3.0_f64.min(turns.len() as f64);
        let total: u64 = turns.iter().sum();
        let waste = if baseline > 0.0 {
            latest / baseline
        } else {
            1.0
        };

        let waste_str = if waste >= 10.0 {
            format!("\x1B[31m{:.0}x !!!\x1B[0m", waste)
        } else if waste >= 5.0 {
            format!("\x1B[31m{:.0}x !\x1B[0m", waste)
        } else if waste >= 2.5 {
            format!("\x1B[33m{:.1}x\x1B[0m", waste)
        } else {
            format!("{:.1}x", waste)
        };

        let bar = waste_bar(&turns, baseline);

        let waste_pad = if waste_str.contains('\x1B') {
            18 + 9
        } else {
            10
        };
        println!(
            "{:<14} {:<8} {:<12} {:<12} {:<waste_pad$} {:<10} {}",
            &session_id[..14.min(session_id.len())],
            turns.len(),
            format_tokens(baseline as u64),
            format_tokens(latest as u64),
            waste_str,
            format_tokens(total),
            bar,
            waste_pad = waste_pad,
        );

        if first {
            if let Some((spike_turn, spike_val)) =
                turns.iter().enumerate().skip(5).max_by_key(|(_, &v)| v)
            {
                hottest = Some((spike_turn + 1, *spike_val, baseline));
            }
            first = false;
        }
    }

    println!();
    println!("BASELINE = avg tokens/turn for first 5 turns");
    println!("LATEST   = avg tokens/turn for last 3 turns");
    println!("WASTE    = LATEST / BASELINE  (1x = healthy, 10x+ = start fresh)");
    println!("TOTAL    = total tokens consumed in session");
    println!("GROWTH   = sparkline per turn  \x1B[33m(yellow = 2.5x+)\x1B[0m  \x1B[31m(red = 5x+)\x1B[0m  first 5 turns = baseline");

    if let Some((turn, val, base)) = hottest {
        println!();
        println!(
            "Biggest spike (most recent session): turn {} — {} ({:.1}x baseline)",
            turn,
            format_tokens(val),
            val as f64 / base
        );
        println!("  Why: context accumulation — each turn re-sends all prior tool results.");
        println!("  Fix: start fresh session, or use git-semantic map+get instead of grep.");
    }

    Ok(())
}

fn parse_session_turns(path: PathBuf) -> Vec<u64> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let mut turns = Vec::new();
    for line in content.lines() {
        if let Ok(r) = serde_json::from_str::<serde_json::Value>(line) {
            if r.get("type").and_then(|t| t.as_str()) == Some("assistant") {
                if let Some(usage) = r.pointer("/message/usage") {
                    let total = usage
                        .get("input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0)
                        + usage
                            .get("output_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0)
                        + usage
                            .get("cache_creation_input_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0)
                        + usage
                            .get("cache_read_input_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                    if total > 0 {
                        turns.push(total);
                    }
                }
            }
        }
    }
    turns
}

fn format_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.0}k", n as f64 / 1_000.0)
    } else {
        format!("{}", n)
    }
}

fn print_subsystem(subsystem: &map::Subsystem, edges: &[map::Edge]) {
    println!("## {} — {}", subsystem.name, subsystem.description);

    // Files that belong to this subsystem
    let subsystem_files: std::collections::HashSet<&str> =
        subsystem.chunks.iter().map(|c| c.file.as_str()).collect();

    // Entry points: files outside this subsystem that call into it
    let mut entry_points: Vec<(&str, &[String])> = edges
        .iter()
        .filter(|e| {
            subsystem_files.contains(e.to.as_str()) && !subsystem_files.contains(e.from.as_str())
        })
        .map(|e| (e.from.as_str(), e.via.as_slice()))
        .collect();
    entry_points.sort_by_key(|(f, _)| *f);
    entry_points.dedup_by_key(|(f, _)| *f);

    if !entry_points.is_empty() {
        println!("  entry points:");
        for (file, via) in &entry_points {
            if via.is_empty() {
                println!("    {}", file);
            } else {
                println!("    {} (via {})", file, via.join(", "));
            }
        }
    }

    for chunk in &subsystem.chunks {
        println!("  {}", chunk.display());
    }
    println!();
}

fn map_command(query: Option<&str>) -> Result<()> {
    let db = db::Database::init().context("Failed to initialize database")?;

    match query {
        None => {
            let subsystems = db
                .all_subsystems()
                .context("Failed to load subsystems from database")?;

            if subsystems.is_empty() {
                println!(
                    "Semantic map is empty. Run `git-semantic index` then `git-semantic hydrate`."
                );
                return Ok(());
            }

            for subsystem in &subsystems {
                let files: Vec<&str> = subsystem.chunks.iter().map(|c| c.file.as_str()).collect();
                let edges = db.edges_into(&files).context("Failed to load edges")?;
                print_subsystem(subsystem, &edges);
            }
        }
        Some(q) => {
            let query_embedding = embed::generate_embedding(q).context("Failed to embed query")?;

            let subsystem = db
                .query_map(&query_embedding)
                .context("Failed to query map")?;

            match subsystem {
                None => println!(
                    "Semantic map is empty. Run `git-semantic index` then `git-semantic hydrate`."
                ),
                Some(subsystem) => {
                    let files: Vec<&str> =
                        subsystem.chunks.iter().map(|c| c.file.as_str()).collect();
                    let edges = db.edges_into(&files).context("Failed to load edges")?;
                    print_subsystem(&subsystem, &edges);
                }
            }
        }
    }

    Ok(())
}

fn get_command(location: &str) -> Result<()> {
    let chunk_ref = map::ChunkRef::parse(location).ok_or_else(|| {
        anyhow::anyhow!(
            "Invalid location '{}'. Expected format: file:start-end (e.g. src/db.rs:12-34)",
            location
        )
    })?;

    let db = db::Database::init().context("Failed to initialize database")?;
    let chunk = db
        .get_chunk_by_location(
            &chunk_ref.file,
            chunk_ref.start_line as i64,
            chunk_ref.end_line as i64,
        )
        .context("Failed to query database")?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No chunks found overlapping {}:{}-{}. Run `git-semantic hydrate` first.",
                chunk_ref.file,
                chunk_ref.start_line,
                chunk_ref.end_line
            )
        })?;

    println!(
        "// {}:{}-{}",
        chunk.file_path, chunk.start_line, chunk.end_line
    );
    println!("{}", chunk.content);

    Ok(())
}

fn dirs_home() -> Result<PathBuf> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .context("HOME environment variable not set")
}

fn to_git_key(key: &str) -> String {
    format!("semantic.{}", key)
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
        let key = to_git_key(key.context("Key required for --unset")?);
        EmbeddingConfig::unset_git_config(&key)?;
        println!("Unset {}", key);
        return Ok(());
    }

    if get {
        let key = to_git_key(key.context("Key required for --get")?);
        if let Some(value) = EmbeddingConfig::get_git_config(&key) {
            println!("{}", value);
        } else {
            anyhow::bail!("Configuration key '{}' not found", key);
        }
        return Ok(());
    }

    if let (Some(key), Some(value)) = (key, value) {
        let key = to_git_key(key);
        EmbeddingConfig::set_git_config(&key, value)?;
        println!("Set {} = {}", key, value);
        return Ok(());
    }

    if let Some(key) = key {
        let key = to_git_key(key);
        if let Some(value) = EmbeddingConfig::get_git_config(&key) {
            println!("{}", value);
        } else {
            anyhow::bail!("Configuration key '{}' not found", key);
        }
        return Ok(());
    }

    EmbeddingConfig::show()?;
    Ok(())
}
