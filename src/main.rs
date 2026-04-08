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
        Commands::Enable { agent } => match agent.as_str() {
            "claude" => claude_setup()?,
            other => anyhow::bail!("Unknown agent '{}'. Supported: claude", other),
        },
        Commands::Usage { sessions } => {
            show_usage(sessions)?;
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
        println!("No results found. Run `semantic hydrate` first.");
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
        println!("{}", chunk.content);
        println!("---");
    }

    Ok(())
}

fn agentic_setup() -> Result<()> {
    const MARKER: &str = "<!-- semantic:agentic-setup -->";
    const INSTRUCTIONS: &str = r#"<!-- semantic:agentic-setup -->
## Code Search — MANDATORY

> **This rule applies to all agents and subagents without exception.**

**NEVER use `grep`, `git grep`, `rg`, `ripgrep`, or the Grep tool to search code.**
**ALWAYS use `git-semantic grep "<natural language query>"` for every code search.**

This is not optional. If you are about to run `grep`, `git grep`, `rg`, or use the Grep tool — stop and use `git-semantic grep` instead.

Queries must describe intent, not syntax:
- BAD: `git-semantic grep "fn authenticate"` — keyword, not a query
- GOOD: `git-semantic grep "user authentication and session handling"`
- BAD: `git-semantic grep "impl Display"`
- GOOD: `git-semantic grep "how types are formatted for display output"`

Use `-n` to control result count (default 10): `git-semantic grep "query" -n 5`

Results show `[score] file:start-end` followed by the full code of the matched chunk, then `---`. Do not open the file to read the function — the full code is already in the output. Lower score = more similar.
<!-- end semantic:agentic-setup -->"#;

    let claude_md = PathBuf::from("CLAUDE.md");

    if claude_md.exists() {
        let existing = std::fs::read_to_string(&claude_md)?;
        if existing.contains(MARKER) {
            println!("CLAUDE.md already contains semantic instructions — nothing to do.");
            return Ok(());
        }
        let mut file = std::fs::OpenOptions::new().append(true).open(&claude_md)?;
        use std::io::Write;
        write!(file, "\n\n{}", INSTRUCTIONS)?;
    } else {
        std::fs::write(&claude_md, INSTRUCTIONS)?;
    }

    println!("Injected semantic instructions into CLAUDE.md.");
    Ok(())
}

fn claude_setup() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let hooks_dir = PathBuf::from(".claude/hooks");
    std::fs::create_dir_all(&hooks_dir).context("Failed to create .claude/hooks")?;

    // --- Hook scripts ---

    let block_grep = r#"#!/bin/bash
INPUT=$(cat)
TOOL_NAME=$(echo "$INPUT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('tool_name',''))")
if [ "$TOOL_NAME" = "Grep" ]; then
  python3 -c "import json; print(json.dumps({'hookSpecificOutput': {'hookEventName': 'PreToolUse', 'permissionDecision': 'deny', 'permissionDecisionReason': 'The Grep tool is blocked. Use git-semantic grep \"<natural language query>\" instead. Describe what the code does, not what it looks like.'}}))"
  exit 0
fi
exit 0
"#;

    let block_bash_grep = r#"#!/bin/bash
INPUT=$(cat)
COMMAND=$(echo "$INPUT" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('tool_input',{}).get('command',''))")
if echo "$COMMAND" | grep -qP '(^|[|;&\s])\s*(grep|rg|git grep)\s' && ! echo "$COMMAND" | grep -qP 'git-semantic\s+grep'; then
  python3 -c "import json; print(json.dumps({'hookSpecificOutput': {'hookEventName': 'PreToolUse', 'permissionDecision': 'deny', 'permissionDecisionReason': 'grep, rg, and git grep are blocked. Use git-semantic grep \"<natural language query>\" instead. Describe what the code does, not what it looks like.'}}))"
  exit 0
fi
exit 0
"#;

    let block_read = r#"#!/bin/bash
INPUT=$(cat)
TOOL_NAME=$(echo "$INPUT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('tool_name',''))")
if [ "$TOOL_NAME" = "Read" ]; then
  python3 -c "import json; print(json.dumps({'hookSpecificOutput': {'hookEventName': 'PreToolUse', 'permissionDecision': 'deny', 'permissionDecisionReason': 'Reading whole files is blocked. Use git-semantic grep \"<natural language query>\" first — it returns the exact function or block you need. Only read a file if semantic search cannot answer the question.'}}))"
  exit 0
fi
exit 0
"#;

    // Monitors token usage per turn and warns on waste factor growth.
    // Reads the session JSONL at ~/.claude/projects/<project>/<session>.jsonl
    // and injects a warning when tokens/turn grows 5x+ from session baseline.
    let token_monitor = r#"#!/usr/bin/env python3
import sys, json, os, glob

def find_transcript(session_id):
    projects_dir = os.path.expanduser("~/.claude/projects")
    if not os.path.isdir(projects_dir):
        return None
    for project_dir in os.listdir(projects_dir):
        path = os.path.join(projects_dir, project_dir, f"{session_id}.jsonl")
        if os.path.exists(path):
            return path
    return None

def parse_turns(transcript_path):
    turns = []
    try:
        with open(transcript_path) as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    r = json.loads(line)
                    if r.get("type") == "assistant" and r.get("message", {}).get("usage"):
                        u = r["message"]["usage"]
                        total = (u.get("input_tokens", 0) + u.get("output_tokens", 0) +
                                 u.get("cache_creation_input_tokens", 0) +
                                 u.get("cache_read_input_tokens", 0))
                        turns.append(total)
                except Exception:
                    pass
    except Exception:
        pass
    return turns

def main():
    raw = sys.stdin.read()
    try:
        hook_input = json.loads(raw)
    except Exception:
        print("{}")
        return

    session_id = hook_input.get("session_id", "")
    if not session_id:
        print("{}")
        return

    transcript = find_transcript(session_id)
    if not transcript:
        print("{}")
        return

    turns = parse_turns(transcript)
    if len(turns) < 5:
        print("{}")
        return

    baseline = sum(turns[:5]) / 5
    current = sum(turns[-3:]) / min(3, len(turns))

    if baseline == 0:
        print("{}")
        return

    waste = current / baseline

    if waste >= 10:
        msg = (f"[git-semantic WARNING]: Token waste factor is {waste:.0f}x — "
               f"turns started at {baseline/1000:.0f}k tokens, now at {current/1000:.0f}k. "
               f"This session is burning quota rapidly. Consider starting a fresh session.")
        print(json.dumps({"additionalContext": msg}))
    elif waste >= 5:
        msg = (f"[git-semantic]: Token usage is {waste:.0f}x higher than session start "
               f"({baseline/1000:.0f}k → {current/1000:.0f}k tokens/turn). "
               f"Cache may be degrading. If this keeps growing, start a fresh session.")
        print(json.dumps({"additionalContext": msg}))
    else:
        print("{}")

main()
"#;

    let scripts: &[(&str, &str)] = &[
        ("block-grep.sh", block_grep),
        ("block-bash-grep.sh", block_bash_grep),
        ("block-read.sh", block_read),
        ("token-monitor.py", token_monitor),
    ];

    for (name, content) in scripts {
        let path = hooks_dir.join(name);
        std::fs::write(&path, content).with_context(|| format!("Failed to write {}", name))?;
        let mut perms = std::fs::metadata(&path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms)?;
        println!("  wrote .claude/hooks/{}", name);
    }

    // --- settings.json ---
    let settings_path = PathBuf::from(".claude/settings.json");
    let marker = "semantic:claude-setup";

    if settings_path.exists() {
        let existing = std::fs::read_to_string(&settings_path)?;
        if existing.contains(marker) {
            println!("settings.json already configured — nothing to do.");
        } else {
            println!(
                "settings.json exists but was not written by semantic — leaving it unchanged."
            );
            println!("Manually merge the hooks from .claude/hooks/ into your settings.json.");
        }
    } else {
        let settings = format!(
            r#"{{
  "_comment": "{}",
  "hooks": {{
    "PreToolUse": [
      {{
        "matcher": "Grep",
        "hooks": [{{ "type": "command", "command": ".claude/hooks/block-grep.sh" }}]
      }},
      {{
        "matcher": "Bash",
        "hooks": [{{ "type": "command", "command": ".claude/hooks/block-bash-grep.sh" }}]
      }},
      {{
        "matcher": "Read",
        "hooks": [{{ "type": "command", "command": ".claude/hooks/block-read.sh" }}]
      }}
    ],
    "PostToolUse": [
      {{
        "matcher": ".*",
        "hooks": [{{ "type": "command", "command": ".claude/hooks/token-monitor.py" }}]
      }}
    ]
  }}
}}
"#,
            marker
        );
        std::fs::write(&settings_path, settings)?;
        println!("  wrote .claude/settings.json");
    }

    // --- CLAUDE.md ---
    agentic_setup()?;

    println!("\nDone. Claude Code will now:");
    println!("  • block grep, rg, git grep — redirect to git-semantic grep");
    println!("  • block whole-file reads — redirect to git-semantic grep");
    println!("  • warn when token usage grows 5x+ from session baseline");
    Ok(())
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
        "{:<14} {:<8} {:<12} {:<12} {:<10} {:<8}",
        "SESSION", "TURNS", "BASELINE", "LATEST", "WASTE", "TOTAL"
    );
    println!("{}", "-".repeat(65));

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
            format!("{:.0}x !!!", waste)
        } else if waste >= 5.0 {
            format!("{:.0}x !", waste)
        } else {
            format!("{:.1}x", waste)
        };

        println!(
            "{:<14} {:<8} {:<12} {:<12} {:<10} {:<8}",
            &session_id[..14.min(session_id.len())],
            turns.len(),
            format_tokens(baseline as u64),
            format_tokens(latest as u64),
            waste_str,
            format_tokens(total),
        );
    }

    println!();
    println!("BASELINE = avg tokens/turn for first 5 turns");
    println!("LATEST   = avg tokens/turn for last 3 turns");
    println!("WASTE    = LATEST / BASELINE  (1x = healthy, 10x+ = start fresh)");
    println!("TOTAL    = total tokens consumed in session");

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
