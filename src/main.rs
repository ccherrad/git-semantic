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

    #[command(about = "Start the MCP server (JSON-RPC over stdio)")]
    Mcp,

    #[command(about = "Show the codebase map or find clusters matching a query")]
    Map {
        #[arg(help = "Natural language query to find matching clusters (optional)")]
        query: Option<String>,
    },

    #[command(about = "Retrieve a file or specific chunk by file and optional line range")]
    Get {
        #[arg(help = "File path (e.g. src/db.rs) or chunk location (e.g. src/db.rs:12-34)")]
        location: String,
        #[arg(
            long,
            help = "Output mode: full (default), signatures (declarations only), outline (names + ranges)"
        )]
        mode: Option<String>,
    },

    #[command(about = "Show a health heatmap of codebase communities")]
    Health {
        #[arg(
            long,
            help = "Drill down into a specific community (partial name match)"
        )]
        community: Option<String>,
    },

    #[command(about = "Benchmark token savings across read modes for indexed files")]
    Benchmark {
        #[arg(long, help = "Output results as JSON")]
        json: bool,
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
        Commands::Health { community } => {
            health_command(community.as_deref())?;
        }
        Commands::Get { location, mode } => {
            get_command(&location, mode.as_deref())?;
        }
        Commands::Mcp => {
            mcp_serve()?;
        }
        Commands::Benchmark { json } => {
            benchmark_command(json)?;
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

        let code_chunks = git_topology::chunking::chunk_code(&content, Some(relative))?;
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

    if !git_topology::EmbeddingConfig::is_provider_configured() {
        anyhow::bail!(
            "topology.provider is not configured.\n\
            Run: git config topology.provider gemma\n\
              or: git config topology.provider openai"
        );
    }
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
            for cluster in &map.clusters {
                db.insert_cluster(cluster)
                    .context("Failed to insert cluster")?;
            }
            for edge in &map.edges {
                db.insert_edge(edge).context("Failed to insert edge")?;
            }
            println!(
                "Loaded map: {} clusters, {} edges.",
                map.clusters.len(),
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

fn mcp_serve() -> Result<()> {
    use std::io::{BufRead, Write};

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) if l.trim().is_empty() => continue,
            Ok(l) => l,
            Err(_) => break,
        };

        let req: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let method = req["method"].as_str().unwrap_or("");

        let response = match method {
            "initialize" => mcp_ok(
                id,
                serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "git-semantic", "version": env!("CARGO_PKG_VERSION") }
                }),
            ),
            "notifications/initialized" => continue,
            "tools/list" => mcp_ok(
                id,
                serde_json::json!({
                    "tools": [
                        {
                            "name": "map",
                            "description": "Orient in the codebase. Returns the most relevant cluster — semantically clustered files with chunk locations and call edges. Use this first.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "query": { "type": "string", "description": "Natural language description of what you are looking for. Omit to list all clusters." }
                                }
                            }
                        },
                        {
                            "name": "get",
                            "description": "Retrieve a file or exact chunk. Use --mode outline (~96% token reduction) to read cheaply, signatures (~86%) for declarations, or omit for full content. Use file:start-end for an exact chunk.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "location": { "type": "string", "description": "File path (e.g. src/db.rs) or chunk location (e.g. src/db.rs:12-34)" },
                                    "mode": { "type": "string", "enum": ["outline", "signatures", "full"], "description": "Output mode. Default: full." }
                                },
                                "required": ["location"]
                            }
                        },
                        {
                            "name": "grep",
                            "description": "Search code using hybrid BM25 + semantic + graph proximity search. Use when map did not surface what you need, or for exact identifier lookups.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "query": { "type": "string", "description": "Natural language query or exact identifier" },
                                    "n": { "type": "integer", "description": "Maximum results. Default: 10." }
                                },
                                "required": ["query"]
                            }
                        },
                        {
                            "name": "health",
                            "description": "Show coupling and fan-in metrics for each cluster. Optionally filter by cluster name.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "community": { "type": "string", "description": "Filter to clusters whose name contains this string (case-insensitive). Omit for all." }
                                }
                            }
                        },
                    ]
                }),
            ),
            "tools/call" => {
                let name = req["params"]["name"].as_str().unwrap_or("");
                let args = &req["params"]["arguments"];
                match mcp_dispatch(name, args) {
                    Ok((text, structured)) => {
                        let mut result = serde_json::json!({
                            "content": [{ "type": "text", "text": text }]
                        });
                        if let Some(data) = structured {
                            result["structuredContent"] = data;
                        }
                        mcp_ok(id, result)
                    }
                    Err(e) => mcp_err(id, -32000, &e.to_string()),
                }
            }
            _ => mcp_err(id, -32601, "method not found"),
        };

        writeln!(out, "{}", serde_json::to_string(&response)?)?;
        out.flush()?;
    }

    Ok(())
}

fn mcp_ok(id: serde_json::Value, result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn mcp_err(id: serde_json::Value, code: i32, msg: &str) -> serde_json::Value {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": msg } })
}

fn mcp_dispatch(
    name: &str,
    args: &serde_json::Value,
) -> Result<(String, Option<serde_json::Value>)> {
    match name {
        "map" => {
            let query = args["query"].as_str();
            let text = mcp_map(query)?;
            Ok((text, None))
        }
        "get" => {
            let location = args["location"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing required argument: location"))?;
            let mode = args["mode"].as_str();
            let text = mcp_get(location, mode)?;
            Ok((text, None))
        }
        "grep" => {
            let query = args["query"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing required argument: query"))?;
            let n = args["n"].as_i64().unwrap_or(10);
            let text = mcp_grep(query, n)?;
            Ok((text, None))
        }
        "health" => {
            let community = args["community"].as_str();
            let data = mcp_health(community)?;
            let text = serde_json::to_string_pretty(&data).unwrap_or_default();
            Ok((text, Some(data)))
        }
        _ => anyhow::bail!("unknown tool: {}", name),
    }
}

fn mcp_map(query: Option<&str>) -> Result<String> {
    let db = db::Database::init()?;
    let mut out = String::new();

    match query {
        None => {
            let clusters = db.all_clusters()?;
            if clusters.is_empty() {
                return Ok(
                    "Semantic map is empty. Run `git-semantic index` then `git-semantic hydrate`."
                        .into(),
                );
            }
            for cluster in &clusters {
                let files: Vec<&str> = cluster.chunks.iter().map(|c| c.file.as_str()).collect();
                let edges = db.edges_into(&files)?;
                out.push_str(&format_cluster(cluster, &edges));
            }
        }
        Some(q) => {
            let embedding = embed::generate_embedding(q)?;
            match db.query_map(&embedding)? {
                None => return Ok(
                    "Semantic map is empty. Run `git-semantic index` then `git-semantic hydrate`."
                        .into(),
                ),
                Some(cluster) => {
                    let files: Vec<&str> = cluster.chunks.iter().map(|c| c.file.as_str()).collect();
                    let edges = db.edges_into(&files)?;
                    out.push_str(&format_cluster(&cluster, &edges));
                }
            }
        }
    }

    Ok(out)
}

fn format_cluster(cluster: &map::Cluster, edges: &[map::Edge]) -> String {
    let mut out = String::new();
    out.push_str(&format!("## {} — {}\n", cluster.name, cluster.description));

    let cluster_files: std::collections::HashSet<&str> =
        cluster.chunks.iter().map(|c| c.file.as_str()).collect();

    let mut entry_points: Vec<(&str, &[String])> = edges
        .iter()
        .filter(|e| {
            cluster_files.contains(e.to.as_str()) && !cluster_files.contains(e.from.as_str())
        })
        .map(|e| (e.from.as_str(), e.via.as_slice()))
        .collect();
    entry_points.sort_by_key(|(f, _)| *f);
    entry_points.dedup_by_key(|(f, _)| *f);

    if !entry_points.is_empty() {
        out.push_str("  entry points:\n");
        for (file, via) in &entry_points {
            if via.is_empty() {
                out.push_str(&format!("    {}\n", file));
            } else {
                out.push_str(&format!("    {} (via {})\n", file, via.join(", ")));
            }
        }
    }

    for chunk in &cluster.chunks {
        out.push_str(&format!("  {}\n", chunk.display()));
    }
    out.push('\n');
    out
}

fn mcp_get(location: &str, mode: Option<&str>) -> Result<String> {
    let db = db::Database::init()?;
    let mut out = String::new();

    if let Some(chunk_ref) = map::ChunkRef::parse(location) {
        let chunk = db
            .get_chunk_by_location(
                &chunk_ref.file,
                chunk_ref.start_line as i64,
                chunk_ref.end_line as i64,
            )?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "No chunks found overlapping {}:{}-{}. Run `git-semantic hydrate` first.",
                    chunk_ref.file,
                    chunk_ref.start_line,
                    chunk_ref.end_line
                )
            })?;
        out.push_str(&format!(
            "// {}:{}-{}\n",
            chunk.file_path, chunk.start_line, chunk.end_line
        ));
        out.push_str(&chunk.content);
    } else {
        let chunks = db.get_chunks_for_file(location)?;
        if chunks.is_empty() {
            anyhow::bail!(
                "No chunks found for '{}'. Run `git-semantic hydrate` first.",
                location
            );
        }

        let entry_points = db.edges_for_file(location).unwrap_or_default();
        out.push_str(&format!("// {}\n", location));
        if !entry_points.is_empty() {
            out.push_str("// callers:\n");
            for e in &entry_points {
                if e.via.is_empty() {
                    out.push_str(&format!("//   {}\n", e.from));
                } else {
                    out.push_str(&format!("//   {} (via {})\n", e.from, e.via.join(", ")));
                }
            }
            out.push('\n');
        }

        match mode.unwrap_or("full") {
            "outline" => {
                for chunk in &chunks {
                    let name = chunk_name(chunk, location);
                    out.push_str(&format!(
                        "  L{}-{}  {}\n",
                        chunk.start_line, chunk.end_line, name
                    ));
                }
            }
            "signatures" => {
                for chunk in &chunks {
                    let sig = chunk_signature(chunk, location);
                    out.push_str(&format!(
                        "{}  // L{}-{}\n\n",
                        sig, chunk.start_line, chunk.end_line
                    ));
                }
            }
            _ => {
                for chunk in &chunks {
                    out.push_str(&chunk.content);
                    out.push('\n');
                }
            }
        }
    }

    Ok(out)
}

fn mcp_grep(query: &str, n: i64) -> Result<String> {
    let db = db::Database::init()?;
    let embedding = embed::generate_embedding(query)?;
    let results = db.search_hybrid(query, &embedding, n)?;

    if results.is_empty() {
        return Ok("No results found. Run `git-semantic hydrate` first.".into());
    }

    let mut out = String::new();
    for chunk in &results {
        let score = chunk
            .distance
            .map(|d| format!("{:.4}", d))
            .unwrap_or_else(|| "N/A".into());
        out.push_str(&format!(
            "[{}] {}:{}-{}\n",
            score, chunk.file_path, chunk.start_line, chunk.end_line
        ));
        out.push_str(&chunk.content);
        out.push_str("\n---\n");
    }
    Ok(out)
}

fn mcp_health(community: Option<&str>) -> Result<serde_json::Value> {
    let db = db::Database::init()?;
    let clusters = db.all_clusters()?;
    if clusters.is_empty() {
        return Ok(serde_json::json!({
            "error": "no_index",
            "hint": "Run `git-semantic index` or `git-semantic hydrate` first."
        }));
    }
    let all_edges = db.all_edges()?;

    let mut rows: Vec<serde_json::Value> = Vec::new();

    for cluster in &clusters {
        let files: Vec<&str> = cluster
            .chunks
            .iter()
            .map(|c| c.file.as_str())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        if let Some(filter) = community {
            if !cluster.name.to_lowercase().contains(&filter.to_lowercase()) {
                continue;
            }
        }

        let file_set: std::collections::HashSet<&str> = files.iter().copied().collect();
        let coupling_out = all_edges
            .iter()
            .filter(|e| file_set.contains(e.from.as_str()) && !file_set.contains(e.to.as_str()))
            .count();
        let fan_in = all_edges
            .iter()
            .filter(|e| file_set.contains(e.to.as_str()) && !file_set.contains(e.from.as_str()))
            .count();

        rows.push(serde_json::json!({
            "name": cluster.name,
            "description": cluster.description,
            "files": files.len(),
            "chunks": cluster.chunks.len(),
            "coupling_out": coupling_out,
            "fan_in": fan_in,
        }));
    }

    Ok(serde_json::json!({ "clusters": rows }))
}

fn map_command(query: Option<&str>) -> Result<()> {
    print!("{}", mcp_map(query)?);
    Ok(())
}

fn health_command(community_filter: Option<&str>) -> Result<()> {
    let db = db::Database::init().context("Failed to open semantic database")?;
    let clusters = db.all_clusters().context("Failed to load clusters")?;

    if clusters.is_empty() {
        anyhow::bail!("No index found. Run `git-semantic index` or `git-semantic hydrate` first.");
    }

    let all_edges = db.all_edges().context("Failed to load edges")?;

    struct CommunityHealth {
        name: String,
        files: usize,
        chunks: usize,
        cohesion: f32,
        coupling_out: usize,
        fan_in: usize,
    }

    let mut rows: Vec<CommunityHealth> = Vec::new();

    for cluster in &clusters {
        let files: Vec<&str> = cluster
            .chunks
            .iter()
            .map(|c| c.file.as_str())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        let file_count = files.len();
        let chunk_count = cluster.chunks.len();

        let file_set: std::collections::HashSet<&str> = files.iter().copied().collect();

        let coupling_out = all_edges
            .iter()
            .filter(|e| file_set.contains(e.from.as_str()) && !file_set.contains(e.to.as_str()))
            .count();

        let fan_in = all_edges
            .iter()
            .filter(|e| file_set.contains(e.to.as_str()) && !file_set.contains(e.from.as_str()))
            .count();

        let embeddings = db.file_embeddings_for(&files).unwrap_or_default();

        let cohesion = if embeddings.len() < 2 {
            1.0f32
        } else {
            let embs: Vec<&Vec<f32>> = embeddings.iter().map(|(_, e)| e).collect();
            let n = embs.len();
            let mut total_sim = 0.0f32;
            let mut count = 0usize;
            for i in 0..n {
                for j in (i + 1)..n {
                    total_sim += 1.0 - clustering::cosine_distance(embs[i], embs[j]);
                    count += 1;
                }
            }
            if count > 0 {
                total_sim / count as f32
            } else {
                1.0
            }
        };

        rows.push(CommunityHealth {
            name: cluster.name.clone(),
            files: file_count,
            chunks: chunk_count,
            cohesion,
            coupling_out,
            fan_in,
        });
    }

    // Drill-down mode
    if let Some(filter) = community_filter {
        let filter_lc = filter.to_lowercase();
        let matched = clusters
            .iter()
            .find(|s| s.name.to_lowercase().contains(&filter_lc));

        let cluster = matched.ok_or_else(|| {
            anyhow::anyhow!(
                "No community matching '{}'. Run `git-semantic health` to list all.",
                filter
            )
        })?;

        let file_set: std::collections::HashSet<&str> =
            cluster.chunks.iter().map(|c| c.file.as_str()).collect();
        let files: Vec<&str> = {
            let mut v: Vec<&str> = file_set.iter().copied().collect();
            v.sort();
            v
        };

        let reset = "\x1b[0m";
        let bold = "\x1b[1m";
        let dim = "\x1b[2m";
        let red = "\x1b[31m";
        let yellow = "\x1b[33m";
        let green = "\x1b[32m";

        println!("\n{bold}{}{reset}", cluster.name);
        println!("{}", "─".repeat(cluster.name.len().max(40)));

        // Metrics for this community
        let row = rows.iter().find(|r| r.name == cluster.name);
        if let Some(r) = row {
            let cohesion_col = if r.cohesion >= 0.75 {
                green
            } else if r.cohesion >= 0.5 {
                yellow
            } else {
                red
            };
            let coup_col = if r.coupling_out <= 2 {
                green
            } else if r.coupling_out <= 6 {
                yellow
            } else {
                red
            };
            let fanin_col = if r.fan_in <= 2 {
                green
            } else if r.fan_in <= 6 {
                yellow
            } else {
                red
            };
            println!(
                "  cohesion  {cohesion_col}{:.2}{reset}   files  {}   chunks  {}   coup-out  {coup_col}{}{reset}   fan-in  {fanin_col}{}{reset}",
                r.cohesion, r.files, r.chunks, r.coupling_out, r.fan_in,
                cohesion_col = cohesion_col, coup_col = coup_col, fanin_col = fanin_col, reset = reset,
            );
        }

        // Files in community
        println!("\n{bold}Files{reset}  {dim}({}){reset}", files.len());
        for f in &files {
            println!("  {}", f);
        }

        // Top fan-in: communities that depend on this one
        let inbound: Vec<&map::Edge> = all_edges
            .iter()
            .filter(|e| file_set.contains(e.to.as_str()) && !file_set.contains(e.from.as_str()))
            .collect();

        if !inbound.is_empty() {
            // Group by source community name
            let mut by_community: std::collections::HashMap<String, Vec<&str>> =
                std::collections::HashMap::new();
            for edge in &inbound {
                let comm_name = clusters
                    .iter()
                    .find(|s| s.chunks.iter().any(|c| c.file == edge.from))
                    .map(|s| s.name.clone())
                    .unwrap_or_else(|| edge.from.clone());
                by_community
                    .entry(comm_name)
                    .or_default()
                    .extend(edge.via.iter().map(|s| s.as_str()));
            }
            let mut sorted: Vec<(String, Vec<&str>)> = by_community.into_iter().collect();
            sorted.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
            sorted.truncate(10);

            println!(
                "\n{bold}Top dependents{reset}  {dim}(communities importing from this one){reset}"
            );
            for (comm, syms) in &sorted {
                let mut deduped = syms.clone();
                deduped.sort();
                deduped.dedup();
                deduped.truncate(5);
                println!(
                    "  {red}{}{reset}  {dim}via {}{reset}",
                    comm,
                    deduped.join(", "),
                    red = red,
                    reset = reset,
                    dim = dim
                );
            }
        }

        // Top fan-out: communities this one depends on
        let outbound: Vec<&map::Edge> = all_edges
            .iter()
            .filter(|e| file_set.contains(e.from.as_str()) && !file_set.contains(e.to.as_str()))
            .collect();

        if !outbound.is_empty() {
            let mut by_community: std::collections::HashMap<String, Vec<&str>> =
                std::collections::HashMap::new();
            for edge in &outbound {
                let comm_name = clusters
                    .iter()
                    .find(|s| s.chunks.iter().any(|c| c.file == edge.to))
                    .map(|s| s.name.clone())
                    .unwrap_or_else(|| edge.to.clone());
                by_community
                    .entry(comm_name)
                    .or_default()
                    .extend(edge.via.iter().map(|s| s.as_str()));
            }
            let mut sorted: Vec<(String, Vec<&str>)> = by_community.into_iter().collect();
            sorted.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
            sorted.truncate(10);

            println!(
                "\n{bold}Top dependencies{reset}  {dim}(communities this one imports from){reset}"
            );
            for (comm, syms) in &sorted {
                let mut deduped = syms.clone();
                deduped.sort();
                deduped.dedup();
                deduped.truncate(5);
                println!(
                    "  {yellow}{}{reset}  {dim}via {}{reset}",
                    comm,
                    deduped.join(", "),
                    yellow = yellow,
                    reset = reset,
                    dim = dim
                );
            }
        }

        println!();
        return Ok(());
    }

    const MIN_CHUNKS: usize = 5;
    let (significant, small): (Vec<_>, Vec<_>) =
        rows.into_iter().partition(|r| r.chunks >= MIN_CHUNKS);
    let rows = significant;

    let max_name = rows
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(10)
        .max(10);
    let bar_width = 20usize;

    fn cohesion_color(c: f32) -> &'static str {
        if c >= 0.75 {
            "\x1b[32m"
        } else if c >= 0.5 {
            "\x1b[33m"
        } else {
            "\x1b[31m"
        }
    }
    fn coupling_color(n: usize) -> &'static str {
        if n <= 2 {
            "\x1b[32m"
        } else if n <= 6 {
            "\x1b[33m"
        } else {
            "\x1b[31m"
        }
    }
    let reset = "\x1b[0m";
    let dim = "\x1b[2m";

    println!(
        "\n{dim}{:<width$}  {:<22}  {:>6}  {:>6}  {:>8}  {:>6}{reset}",
        "COMMUNITY",
        "COHESION",
        "FILES",
        "CHUNKS",
        "COUP-OUT",
        "FAN-IN",
        width = max_name,
        dim = dim,
        reset = reset,
    );
    println!("{}", "─".repeat(max_name + 56));

    for row in &rows {
        let filled = (row.cohesion * bar_width as f32).round() as usize;
        let filled = filled.min(bar_width);
        let empty = bar_width - filled;
        let bar = format!(
            "{}{}{}{}",
            "█".repeat(filled),
            dim,
            "░".repeat(empty),
            reset
        );
        let cc = cohesion_color(row.cohesion);
        let oc = coupling_color(row.coupling_out);
        let ic = coupling_color(row.fan_in);

        println!(
            "{:<width$}  {cc}{bar}{reset}  {dim}{:.2}{reset}  {:>6}  {:>6}  {oc}{:>8}{reset}  {ic}{:>6}{reset}",
            row.name,
            row.cohesion,
            row.files,
            row.chunks,
            row.coupling_out,
            row.fan_in,
            width = max_name,
            cc = cc,
            bar = bar,
            oc = oc,
            ic = ic,
            reset = reset,
            dim = dim,
        );
    }

    if !small.is_empty() {
        println!(
            "{dim}{} small communities (<{} chunks) not shown{reset}",
            small.len(),
            MIN_CHUNKS,
            dim = dim,
            reset = reset,
        );
    }
    println!();
    println!("{dim}cohesion: green ≥0.75  yellow ≥0.50  red <0.50   |   coupling: green ≤2  yellow ≤6  red >6{reset}",
        dim = dim, reset = reset);

    Ok(())
}

fn get_command(location: &str, mode: Option<&str>) -> Result<()> {
    let db = db::Database::init().context("Failed to initialize database")?;

    if let Some(chunk_ref) = map::ChunkRef::parse(location) {
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
    } else {
        let chunks = db
            .get_chunks_for_file(location)
            .context("Failed to query database")?;

        if chunks.is_empty() {
            anyhow::bail!(
                "No chunks found for '{}'. Run `git-semantic hydrate` first.",
                location
            );
        }

        let entry_points = db.edges_for_file(location).unwrap_or_default();

        match mode.unwrap_or("full") {
            "outline" => {
                println!("// {}", location);
                if !entry_points.is_empty() {
                    println!("// callers:");
                    for e in &entry_points {
                        if e.via.is_empty() {
                            println!("//   {}", e.from);
                        } else {
                            println!("//   {} (via {})", e.from, e.via.join(", "));
                        }
                    }
                }
                for chunk in &chunks {
                    let name = chunk_name(chunk, location);
                    println!("  L{}-{}  {}", chunk.start_line, chunk.end_line, name);
                }
            }
            "signatures" => {
                println!("// {}", location);
                if !entry_points.is_empty() {
                    println!("// callers:");
                    for e in &entry_points {
                        if e.via.is_empty() {
                            println!("//   {}", e.from);
                        } else {
                            println!("//   {} (via {})", e.from, e.via.join(", "));
                        }
                    }
                    println!();
                }
                for chunk in &chunks {
                    let sig = chunk_signature(chunk, location);
                    println!("{}  // L{}-{}", sig, chunk.start_line, chunk.end_line);
                    println!();
                }
            }
            _ => {
                println!("// {}", location);
                if !entry_points.is_empty() {
                    println!("// callers:");
                    for e in &entry_points {
                        if e.via.is_empty() {
                            println!("//   {}", e.from);
                        } else {
                            println!("//   {} (via {})", e.from, e.via.join(", "));
                        }
                    }
                    println!();
                }
                for chunk in &chunks {
                    println!("{}", chunk.content);
                }
            }
        }
    }

    Ok(())
}

fn tokens(s: &str) -> usize {
    s.len().div_ceil(4)
}

fn chunk_name(chunk: &models::CodeChunk, file_path: &str) -> String {
    match git_topology::chunking::languages::detect_language(file_path) {
        Some(lang) => git_topology::chunking::parser::extract_name(&chunk.content, lang),
        None => chunk
            .content
            .lines()
            .next()
            .unwrap_or("")
            .trim_end_matches(['{', ':'])
            .trim_end()
            .to_string(),
    }
}

fn outline_tokens(chunks: &[models::CodeChunk]) -> usize {
    let file_path = chunks.first().map(|c| c.file_path.as_str()).unwrap_or("");
    let mut out = String::new();
    for chunk in chunks {
        let name = chunk_name(chunk, file_path);
        out.push_str(&format!(
            "  L{}-{}  {}\n",
            chunk.start_line, chunk.end_line, name
        ));
    }
    tokens(&out)
}

fn chunk_signature(chunk: &models::CodeChunk, file_path: &str) -> String {
    match git_topology::chunking::languages::detect_language(file_path) {
        Some(lang) => git_topology::chunking::parser::extract_signature(&chunk.content, lang),
        None => chunk
            .content
            .lines()
            .next()
            .unwrap_or("")
            .trim_end_matches(['{', ':'])
            .trim_end()
            .to_string(),
    }
}

fn signatures_tokens(chunks: &[models::CodeChunk]) -> usize {
    let file_path = chunks.first().map(|c| c.file_path.as_str()).unwrap_or("");
    let mut out = String::new();
    for chunk in chunks {
        let sig = chunk_signature(chunk, file_path);
        out.push_str(&format!(
            "{}  // L{}-{}\n\n",
            sig, chunk.start_line, chunk.end_line
        ));
    }
    tokens(&out)
}

fn benchmark_command(as_json: bool) -> Result<()> {
    let db = db::Database::init().context("Failed to initialize database")?;
    let clusters = db.all_clusters().context("Failed to load clusters")?;

    if clusters.is_empty() {
        anyhow::bail!(
            "No index found. Run `git-semantic index` then `git-semantic hydrate` first."
        );
    }

    // Collect unique files across all clusters
    let mut all_files: Vec<String> = clusters
        .iter()
        .flat_map(|s| s.chunks.iter().map(|c| c.file.clone()))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    all_files.sort();

    struct FileStats {
        ext: String,
        raw: usize,
        outline: usize,
        signatures: usize,
        full: usize,
    }

    let mut stats: Vec<FileStats> = Vec::new();
    let mut missing = 0usize;

    for file in &all_files {
        let raw_content = match std::fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => {
                missing += 1;
                continue;
            }
        };

        let chunks = db.get_chunks_for_file(file).unwrap_or_default();
        if chunks.is_empty() {
            missing += 1;
            continue;
        }

        let full_text: String = chunks
            .iter()
            .map(|c| c.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let ext = std::path::Path::new(file)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("other")
            .to_string();

        stats.push(FileStats {
            ext,
            raw: tokens(&raw_content),
            outline: outline_tokens(&chunks),
            signatures: signatures_tokens(&chunks),
            full: tokens(&full_text),
        });
    }

    if stats.is_empty() {
        anyhow::bail!("No files could be read. Is the working directory correct?");
    }

    // Group by extension
    let mut by_lang: std::collections::HashMap<String, Vec<&FileStats>> =
        std::collections::HashMap::new();
    for s in &stats {
        by_lang.entry(s.ext.clone()).or_default().push(s);
    }
    let mut lang_rows: Vec<(String, usize, usize, usize, usize, usize)> = by_lang
        .into_iter()
        .map(|(lang, files)| {
            let raw: usize = files.iter().map(|f| f.raw).sum();
            let outline: usize = files.iter().map(|f| f.outline).sum();
            let sigs: usize = files.iter().map(|f| f.signatures).sum();
            let full: usize = files.iter().map(|f| f.full).sum();
            let n = files.len();
            (lang, n, raw, outline, sigs, full)
        })
        .collect();
    lang_rows.sort_by(|a, b| b.2.cmp(&a.2));

    let total_raw: usize = stats.iter().map(|s| s.raw).sum();
    let total_outline: usize = stats.iter().map(|s| s.outline).sum();
    let total_sigs: usize = stats.iter().map(|s| s.signatures).sum();
    let total_full: usize = stats.iter().map(|s| s.full).sum();

    // Session simulation:
    // Assume agent navigates 10 files per session
    // Raw: reads all 10 files fully
    // grep-only: gets 10 chunks avg 80 tokens each
    // map+outline+get: 1 map call (200t) + outline per file (avg) + 3 exact chunks (80t each)
    let sample_n = 10usize;
    let avg_raw = total_raw / stats.len().max(1);
    let avg_outline = total_outline / stats.len().max(1);
    let avg_sigs = total_sigs / stats.len().max(1);
    let avg_chunk = 80usize;
    let map_cost = 300usize;

    let session_raw = avg_raw * sample_n;
    let session_grep = avg_chunk * 10 * sample_n;
    let session_outline = map_cost + (avg_outline + avg_chunk * 3) * sample_n;
    let session_sigs = map_cost + (avg_sigs + avg_chunk * 3) * sample_n;

    fn fmt_tok(n: usize) -> String {
        if n >= 1_000_000 {
            format!("{:.1}M", n as f64 / 1_000_000.0)
        } else if n >= 1_000 {
            format!("{:.0}K", n as f64 / 1_000.0)
        } else {
            format!("{}", n)
        }
    }

    fn cost(n: usize) -> f64 {
        n as f64 * 3.0 / 1_000_000.0
    }

    fn savings(baseline: usize, compressed: usize) -> f64 {
        if baseline == 0 {
            return 0.0;
        }
        (1.0 - compressed as f64 / baseline as f64) * 100.0
    }

    if as_json {
        println!("{{");
        println!("  \"files\": {},", stats.len());
        println!("  \"total_raw_tokens\": {},", total_raw);
        println!("  \"total_outline_tokens\": {},", total_outline);
        println!("  \"total_signatures_tokens\": {},", total_sigs);
        println!(
            "  \"outline_savings_pct\": {:.1},",
            savings(total_raw, total_outline)
        );
        println!(
            "  \"signatures_savings_pct\": {:.1},",
            savings(total_raw, total_sigs)
        );
        println!("  \"by_language\": [");
        for (i, (lang, n, raw, outline, sigs, _)) in lang_rows.iter().enumerate() {
            let comma = if i + 1 < lang_rows.len() { "," } else { "" };
            println!(
                "    {{\"lang\": \"{}\", \"files\": {}, \"raw\": {}, \"outline\": {}, \"signatures\": {}, \"outline_savings\": {:.1}, \"signatures_savings\": {:.1}}}{}",
                lang, n, raw, outline, sigs,
                savings(*raw, *outline), savings(*raw, *sigs), comma
            );
        }
        println!("  ],");
        println!("  \"session_simulation\": {{");
        println!("    \"raw_tokens\": {},", session_raw);
        println!("    \"grep_only_tokens\": {},", session_grep);
        println!("    \"map_outline_get_tokens\": {},", session_outline);
        println!("    \"map_signatures_get_tokens\": {}", session_sigs);
        println!("  }}");
        println!("}}");
        return Ok(());
    }

    let colors = Colors {
        dim: "\x1b[2m",
        bold: "\x1b[1m",
        green: "\x1b[32m",
        yellow: "\x1b[33m",
        reset: "\x1b[0m",
    };
    let Colors {
        dim,
        bold,
        green,
        yellow,
        reset,
    } = &colors;

    println!(
        "\n{bold}Token savings by language{reset}  {dim}({} files, {} missing){reset}\n",
        stats.len(),
        missing
    );
    println!(
        "{dim}{:<8}  {:>6}  {:>8}  {:>10}  {:>12}  {:>10}  {:>10}{reset}",
        "LANG",
        "FILES",
        "RAW",
        "OUTLINE",
        "OUTLINE SAV",
        "SIGS",
        "SIGS SAV",
        dim = dim,
        reset = reset
    );
    println!("{}", "─".repeat(76));

    for (lang, n, raw, outline, sigs, _) in &lang_rows {
        let osav = savings(*raw, *outline);
        let ssav = savings(*raw, *sigs);
        let ocol = if osav >= 80.0 {
            green
        } else if osav >= 50.0 {
            yellow
        } else {
            ""
        };
        let scol = if ssav >= 80.0 {
            green
        } else if ssav >= 50.0 {
            yellow
        } else {
            ""
        };
        println!(
            "{:<8}  {:>6}  {:>8}  {:>10}  {}{:>10.1}%{}  {:>10}  {}{:>9.1}%{}",
            lang,
            n,
            fmt_tok(*raw),
            fmt_tok(*outline),
            ocol,
            osav,
            reset,
            fmt_tok(*sigs),
            scol,
            ssav,
            reset,
        );
    }
    println!("{}", "─".repeat(76));
    let osav = savings(total_raw, total_outline);
    let ssav = savings(total_raw, total_sigs);
    println!(
        "{bold}{:<8}  {:>6}  {:>8}  {:>10}  {:>10.1}%   {:>10}  {:>9.1}%{reset}",
        "TOTAL",
        stats.len(),
        fmt_tok(total_raw),
        fmt_tok(total_outline),
        osav,
        fmt_tok(total_sigs),
        ssav,
        bold = bold,
        reset = reset,
    );

    println!("\n{bold}Read mode comparison{reset}  {dim}(full = all chunks concatenated){reset}\n");
    println!(
        "{dim}{:<14}  {:>10}  {:>10}{reset}",
        "MODE",
        "TOKENS",
        "VS RAW",
        dim = dim,
        reset = reset
    );
    println!("{}", "─".repeat(38));
    println!("{:<14}  {:>10}  {:>9.1}%", "raw", fmt_tok(total_raw), 0.0);
    println!(
        "{:<14}  {:>10}  {}{:>9.1}%{}",
        "full",
        fmt_tok(total_full),
        yellow,
        savings(total_raw, total_full),
        reset
    );
    println!(
        "{:<14}  {:>10}  {}{:>9.1}%{}",
        "signatures",
        fmt_tok(total_sigs),
        green,
        savings(total_raw, total_sigs),
        reset
    );
    println!(
        "{:<14}  {:>10}  {}{:>9.1}%{}",
        "outline",
        fmt_tok(total_outline),
        green,
        savings(total_raw, total_outline),
        reset
    );

    println!(
        "\n{bold}Session simulation{reset}  {dim}({} files × {} navigated, $3/1M tokens){reset}\n",
        stats.len(),
        sample_n
    );
    println!(
        "{dim}{:<28}  {:>8}  {:>8}  {:>8}{reset}",
        "SCENARIO",
        "TOKENS",
        "COST",
        "SAVINGS",
        dim = dim,
        reset = reset
    );
    println!("{}", "─".repeat(58));
    println!(
        "{:<28}  {:>8}  {:>7}  {:>7}",
        "raw (read whole files)",
        fmt_tok(session_raw),
        format!("${:.3}", cost(session_raw)),
        "—"
    );
    println!(
        "{:<28}  {:>8}  {:>7}  {}{:>6.1}%{}",
        "grep only",
        fmt_tok(session_grep),
        format!("${:.3}", cost(session_grep)),
        yellow,
        savings(session_raw, session_grep),
        reset
    );
    println!(
        "{:<28}  {:>8}  {:>7}  {}{:>6.1}%{}",
        "map + outline + get",
        fmt_tok(session_outline),
        format!("${:.3}", cost(session_outline)),
        green,
        savings(session_raw, session_outline),
        reset
    );
    println!(
        "{:<28}  {:>8}  {:>7}  {}{:>6.1}%{}",
        "map + signatures + get",
        fmt_tok(session_sigs),
        format!("${:.3}", cost(session_sigs)),
        green,
        savings(session_raw, session_sigs),
        reset
    );

    println!(
        "\n{dim}outline = first line of each chunk (declaration only)",
        dim = dim
    );
    println!("signatures = declaration block without body");
    println!(
        "get = exact chunk by line range (avg {}t){reset}",
        avg_chunk,
        reset = reset
    );

    // Navigation comparison — replay actual queries against the index
    run_navigation_benchmark(&db, &clusters, as_json, &colors)?;

    Ok(())
}

struct Colors<'a> {
    dim: &'a str,
    bold: &'a str,
    green: &'a str,
    yellow: &'a str,
    reset: &'a str,
}

fn run_navigation_benchmark(
    db: &db::Database,
    clusters: &[map::Cluster],
    as_json: bool,
    colors: &Colors,
) -> Result<()> {
    let Colors {
        dim,
        bold,
        green,
        yellow,
        reset,
    } = colors;
    let sub_embeddings = match db.cluster_embeddings() {
        Ok(e) if !e.is_empty() => e,
        Ok(_) => {
            if !as_json {
                println!(
                    "\n{dim}Navigation benchmark skipped — cluster embeddings empty.{reset}",
                    dim = dim,
                    reset = reset
                );
            }
            return Ok(());
        }
        Err(e) => {
            if !as_json {
                println!(
                    "\n{dim}Navigation benchmark skipped — {}.{reset}",
                    e,
                    dim = dim,
                    reset = reset
                );
            }
            return Ok(());
        }
    };

    // Sample up to 10 clusters spread evenly across the index
    let total = sub_embeddings.len();
    let sample_n = 10.min(total);
    let step = total / sample_n;
    let samples: Vec<&(String, String, Vec<f32>)> =
        (0..sample_n).map(|i| &sub_embeddings[i * step]).collect();

    struct NavResult {
        cluster: String,
        grep_tokens: usize,
        grep_precision: bool,
        outline_tokens: usize,
        outline_precision: bool,
        sigs_tokens: usize,
        sigs_precision: bool,
    }

    let avg_chunk_t = 80usize;
    let map_output_t = 300usize;
    let top_grep = 5usize;
    let top_get = 3usize;

    let mut results: Vec<NavResult> = Vec::new();

    for (name, _desc, embedding) in &samples {
        // Ground truth: files belonging to this cluster
        let ground_truth_files: std::collections::HashSet<String> = clusters
            .iter()
            .find(|s| &s.name == name)
            .map(|s| s.chunks.iter().map(|c| c.file.clone()).collect())
            .unwrap_or_default();

        // Strategy A: grep only — search_similar top 5, count content tokens
        let grep_results = db
            .search_similar(embedding, top_grep as i64)
            .unwrap_or_default();
        let grep_tokens: usize = grep_results.iter().map(|c| tokens(&c.content)).sum();
        let grep_precision = grep_results
            .first()
            .map(|c| ground_truth_files.contains(&c.file_path))
            .unwrap_or(false);

        // Strategy B: map → outline all cluster files → get top 3 chunks
        // map output is the cluster description + chunk list (fixed cost)
        // outline: generate outline for all files in the matched cluster
        let matched_cluster = clusters.iter().find(|s| &s.name == name);
        let (outline_tokens, outline_precision, sigs_tokens, sigs_precision) =
            if let Some(sub) = matched_cluster {
                let sub_files: Vec<String> = sub
                    .chunks
                    .iter()
                    .map(|c| c.file.clone())
                    .collect::<std::collections::HashSet<_>>()
                    .into_iter()
                    .collect();

                let precision = sub_files.iter().any(|f| ground_truth_files.contains(f));

                let mut ol_total = map_output_t;
                let mut sig_total = map_output_t;

                for file in &sub_files {
                    let chunks = db.get_chunks_for_file(file).unwrap_or_default();
                    ol_total += outline_tokens(&chunks);
                    sig_total += signatures_tokens(&chunks);
                }
                // Add cost of top_get exact chunk retrievals
                ol_total += avg_chunk_t * top_get;
                sig_total += avg_chunk_t * top_get;

                (ol_total, precision, sig_total, precision)
            } else {
                (
                    map_output_t + avg_chunk_t * top_get,
                    false,
                    map_output_t + avg_chunk_t * top_get,
                    false,
                )
            };

        results.push(NavResult {
            cluster: name.clone(),
            grep_tokens,
            grep_precision,
            outline_tokens,
            outline_precision,
            sigs_tokens,
            sigs_precision,
        });
    }

    let avg_grep: usize =
        results.iter().map(|r| r.grep_tokens).sum::<usize>() / results.len().max(1);
    let avg_outline: usize =
        results.iter().map(|r| r.outline_tokens).sum::<usize>() / results.len().max(1);
    let avg_sigs: usize =
        results.iter().map(|r| r.sigs_tokens).sum::<usize>() / results.len().max(1);
    let grep_precision_pct =
        results.iter().filter(|r| r.grep_precision).count() * 100 / results.len().max(1);
    let outline_precision_pct =
        results.iter().filter(|r| r.outline_precision).count() * 100 / results.len().max(1);
    let sigs_precision_pct =
        results.iter().filter(|r| r.sigs_precision).count() * 100 / results.len().max(1);
    let total_grep: usize = results.iter().map(|r| r.grep_tokens).sum();
    let total_outline: usize = results.iter().map(|r| r.outline_tokens).sum();
    let total_sigs: usize = results.iter().map(|r| r.sigs_tokens).sum();

    fn fmt_tok(n: usize) -> String {
        if n >= 1_000_000 {
            format!("{:.1}M", n as f64 / 1_000_000.0)
        } else if n >= 1_000 {
            format!("{:.0}K", n as f64 / 1_000.0)
        } else {
            format!("{}", n)
        }
    }

    fn savings(baseline: usize, compressed: usize) -> f64 {
        if baseline == 0 {
            return 0.0;
        }
        (1.0 - compressed as f64 / baseline as f64) * 100.0
    }

    fn cost(n: usize) -> f64 {
        n as f64 * 3.0 / 1_000_000.0
    }

    if as_json {
        println!(",");
        println!("  \"navigation_benchmark\": {{");
        println!("    \"sample_queries\": {},", results.len());
        println!("    \"grep_avg_tokens\": {},", avg_grep);
        println!("    \"grep_precision_pct\": {},", grep_precision_pct);
        println!("    \"map_outline_get_avg_tokens\": {},", avg_outline);
        println!(
            "    \"map_outline_get_precision_pct\": {},",
            outline_precision_pct
        );
        println!("    \"map_signatures_get_avg_tokens\": {},", avg_sigs);
        println!(
            "    \"map_signatures_get_precision_pct\": {},",
            sigs_precision_pct
        );
        println!(
            "    \"outline_token_reduction_pct\": {:.1}",
            savings(total_grep, total_outline)
        );
        println!("  }}");
        return Ok(());
    }

    println!(
        "\n{bold}Navigation comparison{reset}  {dim}({} sample queries, one per cluster){reset}\n",
        results.len()
    );
    println!(
        "{dim}{:<30}  {:>12}  {:>10}  {:>8}{reset}",
        "STRATEGY",
        "AVG TOKENS",
        "PRECISION",
        "TOTAL",
        dim = dim,
        reset = reset
    );
    println!("{}", "─".repeat(66));
    println!(
        "{:<30}  {:>12}  {}{:>9}%{}  {:>8}",
        "grep only (top 5)",
        fmt_tok(avg_grep),
        if grep_precision_pct >= 80 {
            green
        } else {
            yellow
        },
        grep_precision_pct,
        reset,
        fmt_tok(total_grep),
    );
    println!(
        "{:<30}  {:>12}  {}{:>9}%{}  {:>8}",
        "map + outline + get",
        fmt_tok(avg_outline),
        if outline_precision_pct >= 80 {
            green
        } else {
            yellow
        },
        outline_precision_pct,
        reset,
        fmt_tok(total_outline),
    );
    println!(
        "{:<30}  {:>12}  {}{:>9}%{}  {:>8}",
        "map + signatures + get",
        fmt_tok(avg_sigs),
        if sigs_precision_pct >= 80 {
            green
        } else {
            yellow
        },
        sigs_precision_pct,
        reset,
        fmt_tok(total_sigs),
    );
    println!("{}", "─".repeat(66));

    let best_tok = avg_outline.min(avg_sigs);
    let best_label = if avg_outline <= avg_sigs {
        "map + outline + get"
    } else {
        "map + signatures + get"
    };
    let best_precision = if avg_outline <= avg_sigs {
        outline_precision_pct
    } else {
        sigs_precision_pct
    };
    let tok_delta = savings(avg_grep, best_tok);

    println!(
        "\n{bold}Best strategy:{reset} {green}{}{reset}",
        best_label,
        bold = bold,
        green = green,
        reset = reset
    );
    println!(
        "  Precision advantage vs grep:  {green}+{}%{reset}  ({} → {}%)",
        best_precision.saturating_sub(grep_precision_pct),
        grep_precision_pct,
        best_precision,
        green = green,
        reset = reset
    );
    if tok_delta >= 0.0 {
        println!(
            "  Token reduction vs grep:      {green}{:.1}%{reset}",
            tok_delta,
            green = green,
            reset = reset
        );
    } else {
        println!(
            "  Token overhead vs grep:       {yellow}{:.1}%{reset}  {dim}(larger clusters, higher precision){reset}",
            tok_delta.abs(), yellow = yellow, dim = dim, reset = reset
        );
    }
    println!(
        "  Cost per session (×10):       ${:.4}  vs grep ${:.4}",
        cost(best_tok * 10),
        cost(avg_grep * 10)
    );
    println!(
        "\n{dim}Precision = top result belongs to the correct cluster\n\
         Clusters sampled: {}{reset}",
        results
            .iter()
            .map(|r| r.cluster.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        dim = dim,
        reset = reset
    );

    Ok(())
}

fn to_git_key(key: &str) -> String {
    format!("topology.{}", key)
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
