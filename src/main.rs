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

    let repo_path = PathBuf::from(".");
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

fn strip_between(text: &str, start_marker: &str, end_marker: &str) -> String {
    let mut result = String::new();
    let mut skip = false;
    for line in text.lines() {
        if line.contains(start_marker) {
            skip = true;
            continue;
        }
        if skip && line.contains(end_marker) {
            skip = false;
            continue;
        }
        if !skip {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

fn agentic_setup() -> Result<()> {
    const MARKER: &str = "<!-- semantic:agentic-setup -->";
    const INSTRUCTIONS: &str = r#"<!-- semantic:agentic-setup -->
## Code Navigation — MANDATORY

> **This rule applies to all agents and subagents without exception.**

**NEVER use `grep`, `git grep`, `rg`, `ripgrep`, the Grep tool, or whole-file reads to explore code.**
**Use the three-command workflow below for ALL code navigation.**

---

### Critical rules

**1. Retrieve per task, not upfront.**
NEVER load code before you need it. NEVER bulk-read to "build context". Each task gets only the chunks it needs — retrieve, answer, move on.

**2. The map output IS the answer. Do not re-search what the map already told you.**
If `map` shows `embed: generate_embedding, create_provider` — you already know `create_provider` is in `embed.rs`. Use `get` immediately. Do NOT run `grep` to find it again.

**3. If the map description contains the function/type name you need — stop. Use `get` on that chunk.**
Map descriptions are derived from the actual code. A name in the description means the chunk contains it.

**4. Do not use TodoWrite, TaskCreate, or TaskUpdate for sequential task lists.**
Work through tasks one by one without planning overhead. Read the next task, do it, write the result, move to the next.

**5. Maximum 3 `get` calls per task.**
If you need more than 3 chunks for one task, you are over-reading. The answer is in fewer chunks than you think.

**6. Never re-fetch a chunk already in context.**
If you already retrieved `src/db.rs:10-169` for task 2, do not retrieve it again for task 14. It is already in your context.

---

### The workflow (repeat per task, no planning phase)

**Step 1 — Orient**
```bash
git-semantic map "<natural language query>"
```
Read the output. If it names the function/type you need — skip to step 2 immediately.

**Step 2 — Get only what this task needs**
```bash
git-semantic get <file:start-end>
```
Use the locations from the map output directly. Max 3 calls.

**Step 3 — Search only if map description did not contain what you need**
```bash
git-semantic grep "<natural language query>"
```
Last resort. If the map named the thing, this step is skipped entirely.

---

### Priority order

1. `git-semantic map "<query>"` — orient, read output carefully
2. `git-semantic get <file:start-end>` — use map locations directly (max 3)
3. `git-semantic grep "<query>"` — only if map was truly insufficient

Lower score = more similar in grep results.
<!-- end semantic:agentic-setup -->"#;

    let claude_md = PathBuf::from("CLAUDE.md");
    const OLD_MARKER: &str = "<!-- gitsem:agentic-setup -->";

    if claude_md.exists() {
        let existing = std::fs::read_to_string(&claude_md)?;
        if existing.contains(MARKER) {
            println!("CLAUDE.md already contains semantic instructions — nothing to do.");
            return Ok(());
        }
        // Replace old marker block entirely if present, otherwise append
        if existing.contains(OLD_MARKER) {
            // Strip everything between old markers and write fresh
            let stripped =
                strip_between(&existing, OLD_MARKER, "<!-- end gitsem:agentic-setup -->");
            let new_content = format!("{}\n\n{}", stripped.trim(), INSTRUCTIONS);
            std::fs::write(&claude_md, new_content)?;
        } else {
            let mut file = std::fs::OpenOptions::new().append(true).open(&claude_md)?;
            use std::io::Write;
            write!(file, "\n\n{}", INSTRUCTIONS)?;
        }
    } else {
        std::fs::write(&claude_md, INSTRUCTIONS)?;
    }

    println!("Injected semantic instructions into CLAUDE.md.");
    Ok(())
}

fn claude_setup() -> Result<()> {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    let hooks_dir = PathBuf::from(".claude/hooks");
    std::fs::create_dir_all(&hooks_dir).context("Failed to create .claude/hooks")?;

    // --- Hook scripts ---

    let block_grep = r#"#!/bin/bash
INPUT=$(cat)
TOOL_NAME=$(echo "$INPUT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('tool_name',''))")
if [ "$TOOL_NAME" = "Grep" ]; then
  python3 -c "import json; print(json.dumps({'hookSpecificOutput': {'hookEventName': 'PreToolUse', 'permissionDecision': 'deny', 'permissionDecisionReason': 'The Grep tool is blocked. Use the three-command workflow: (1) git-semantic map \"<query>\" to orient, (2) git-semantic get <file:start-end> to read a known chunk, (3) git-semantic grep \"<query>\" only if map is insufficient.'}}))"
  exit 0
fi
exit 0
"#;

    let block_bash_grep = r#"#!/bin/bash
INPUT=$(cat)
COMMAND=$(echo "$INPUT" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('tool_input',{}).get('command',''))")
if echo "$COMMAND" | grep -qP '(^|[|;&\s])\s*(grep|rg|git grep)\s' && ! echo "$COMMAND" | grep -qP 'git-semantic\s+(grep|map|get)'; then
  python3 -c "import json; print(json.dumps({'hookSpecificOutput': {'hookEventName': 'PreToolUse', 'permissionDecision': 'deny', 'permissionDecisionReason': 'grep, rg, and git grep are blocked. Use the three-command workflow: (1) git-semantic map \"<query>\" to orient, (2) git-semantic get <file:start-end> to read a known chunk, (3) git-semantic grep \"<query>\" only if map is insufficient.'}}))"
  exit 0
fi
exit 0
"#;

    let block_read = r#"#!/bin/bash
INPUT=$(cat)
TOOL_NAME=$(echo "$INPUT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('tool_name',''))")
if [ "$TOOL_NAME" = "Read" ]; then
  python3 -c "import json; print(json.dumps({'hookSpecificOutput': {'hookEventName': 'PreToolUse', 'permissionDecision': 'deny', 'permissionDecisionReason': 'Reading whole files is blocked. Use the three-command workflow: (1) git-semantic map \"<query>\" to orient and find the subsystem + entry points, (2) git-semantic get <file:start-end> to read a specific chunk, (3) git-semantic grep \"<query>\" only if map is insufficient.'}}))"
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

    let capture_session = r#"#!/bin/bash
INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('session_id',''))" 2>/dev/null)
if [ -n "$SESSION_ID" ]; then
  git-semantic session capture --session-id "$SESSION_ID" 2>/dev/null || true
  git push origin cognitive-debt/v1 2>/dev/null || true
fi
exit 0
"#;

    let scripts: &[(&str, &str)] = &[
        ("block-grep.sh", block_grep),
        ("block-bash-grep.sh", block_bash_grep),
        ("block-read.sh", block_read),
        ("token-monitor.py", token_monitor),
        ("capture-session.sh", capture_session),
    ];

    for (name, content) in scripts {
        let path = hooks_dir.join(name);
        std::fs::write(&path, content).with_context(|| format!("Failed to write {}", name))?;
        #[cfg(unix)]
        {
            let mut perms = std::fs::metadata(&path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms)?;
        }
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
    ],
    "Stop": [
      {{
        "hooks": [{{ "type": "command", "command": ".claude/hooks/capture-session.sh" }}]
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

    // --- git post-commit hook ---
    let git_hooks_dir = PathBuf::from(".git/hooks");
    if git_hooks_dir.exists() {
        let post_commit_path = git_hooks_dir.join("post-commit");
        let post_commit_script = "#!/bin/bash\ngit-semantic audit --commit HEAD\ngit push origin cognitive-debt/v1 2>/dev/null || true\n";

        let should_write = if post_commit_path.exists() {
            let existing = std::fs::read_to_string(&post_commit_path).unwrap_or_default();
            !existing.contains("git-semantic audit")
        } else {
            true
        };

        if should_write {
            if post_commit_path.exists() {
                let existing = std::fs::read_to_string(&post_commit_path).unwrap_or_default();
                let appended = format!(
                    "{}\n# git-semantic cognitive debt audit\ngit-semantic audit --commit HEAD\ngit push origin cognitive-debt/v1 2>/dev/null || true\n",
                    existing.trim()
                );
                std::fs::write(&post_commit_path, appended)?;
            } else {
                std::fs::write(&post_commit_path, post_commit_script)?;
            }
            #[cfg(unix)]
            {
                let mut perms = std::fs::metadata(&post_commit_path)?.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&post_commit_path, perms)?;
            }
            println!("  wrote .git/hooks/post-commit");
        } else {
            println!("  .git/hooks/post-commit already configured — nothing to do.");
        }
    }

    // --- CLAUDE.md ---
    agentic_setup()?;

    println!("\nDone. Claude Code will now:");
    println!("  • block grep, rg, git grep — redirect to git-semantic grep");
    println!("  • block whole-file reads — redirect to git-semantic grep");
    println!("  • warn when token usage grows 5x+ from session baseline");
    println!("  • auto-audit every commit for cognitive debt (post-commit hook)");
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
