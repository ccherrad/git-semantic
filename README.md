# git-semantic

> Semantic search and spatial navigation for Git repositories — so AI coding agents orient in one turn and retrieve exactly what they need.

`git-semantic` parses every tracked file with tree-sitter, generates vector embeddings per chunk, and stores them on a dedicated orphan Git branch. At index time it also builds a **spatial map** of the codebase — grouping files into semantically coherent subsystems using Leiden community detection, labeling them by their key functions, and tracking cross-file call edges.

Search is hybrid: BM25 (SQLite FTS5) + semantic embeddings + graph proximity, merged via Reciprocal Rank Fusion. Exact identifier lookups score higher when they appear in both ranked lists; files connected via call edges to top results are boosted automatically.

---

## Motivation

Good agents don't need to explore — they need to know where to look and how much to read.

`git-semantic` gives agents a spatial model of the codebase. Instead of searching and accumulating, an agent can orient with `map`, read a file's structure with `get --mode outline` (~96% token reduction), pull the declaration with `--mode signatures` (~86% reduction), and fetch the exact body with `get file:start-end` only when it needs to. A well-structured session stays flat — no context bloat, no waste ratio inflation — because the agent fetches surgically from the start.

The index lives on a Git branch. One person indexes, the whole team benefits — no re-embedding, no API keys per developer. The map is shared state: every agent session starts with the same orientation, not a cold rediscovery of the codebase.

`git-semantic benchmark` measures this concretely on your own repo: token savings per language, read mode comparison, and a navigation replay that shows grep precision vs map+outline+get precision across sampled subsystems.

### Benchmark — [Textual](https://github.com/Textualize/textual) (988 Python files)

**Token savings by read mode**

| mode | tokens | vs raw |
|------|--------|--------|
| raw | 1.1M | — |
| full (chunks) | 1.1M | 4.3% |
| signatures | 152K | 86.4% |
| outline | 41K | **96.3%** |

**Session simulation** (10 files navigated, $3/1M tokens)

| scenario | tokens | cost | savings |
|----------|--------|------|---------|
| raw (read whole files) | 11K | $0.034 | — |
| grep only | 8K | $0.024 | 28.8% |
| map + outline + get | 3K | $0.009 | **72.3%** |
| map + signatures + get | 4K | $0.013 | 62.4% |

**Navigation comparison** (10 sampled subsystem queries)

| strategy | avg tokens/query | precision |
|----------|-----------------|-----------|
| grep only (top 5) | 377 | 40% |
| map + outline + get | 2K | **100%** |
| map + signatures + get | 2K | **100%** |

Precision = top result belongs to the correct subsystem.

The session simulation assumes one query, fixed chunk count. Real agents don't work that way — Claude Code reads however many results look relevant (typically 2-3, sometimes more) and retries if nothing fits. The token numbers are a lower bound, not the full picture.

What the simulation doesn't capture: at 40% precision, 6 in 10 grep queries land in the wrong subsystem. The agent reads wrong chunks, backtracks, searches again — each retry compounds context. map + outline always lands on the first try, so the end-to-end cost is lower even though the per-query token count is higher. Precision is the metric that matters; the token counts are illustrative.

---

## How it works

```
main branch                   semantic branch (orphan)
──────────────────            ──────────────────────────
src/main.rs          →        src/main.rs         ← [{start_line, end_line, text, embedding}, ...]
src/db.rs            →        src/db.rs           ← [{...}, ...]
src/chunking/mod.rs  →        src/chunking/mod.rs
                              .semantic-map.json  ← subsystems + edges
```

1. `git-semantic index` parses all tracked files, embeds each chunk, clusters files into subsystems using Leiden community detection, builds the spatial map, and commits everything to the `semantic` orphan branch.
2. `git push origin semantic` shares the embeddings and map with the team.
3. Everyone else runs `git fetch origin semantic` + `git-semantic hydrate` to populate their local SQLite search index (vector + FTS5) — no re-embedding needed.
4. Agents use `map` to orient, `get --mode outline` to read cheaply, `get file:start-end` to retrieve exactly, and `grep` only when the map is insufficient.

---

## Installation

```bash
cargo install gitsem
```

**Prerequisites:** Rust 1.65+, Git 2.0+

---

## Commands

### `git-semantic index`

Parses and embeds all tracked files, builds the spatial map, and commits to the `semantic` branch.

- First run: full index
- Subsequent runs: incremental — only changed files are re-embedded
- Respects `.gitignore`
- Skips binary files

### `git-semantic hydrate`

Reads the `semantic` branch and populates the local `.git/semantic.db` index. Fetches `origin/semantic` first, falls back to local.

### `git-semantic map [query]`

Show the spatial map of the codebase, or find the subsystem relevant to a task. Subsystems are built by Leiden community detection — files are grouped by embedding similarity, not filesystem location, so semantically related files cluster together even in flat repos.

```bash
git-semantic map
# → lists all subsystems with key functions and entry points

git-semantic map "where does embedding dispatch happen"
# → returns the most relevant subsystem with file locations and call edges
```

Output:

```
## embeddings — gemma: GemmaProvider, EmbeddingConfig, cache_dir, TextEmbedding
  entry points:
    src/embed.rs (via create_provider, EmbeddingConfig)
    src/main.rs (via EmbeddingConfig, load_or_default)
  src/embeddings/gemma.rs:1-45
  src/embeddings/config.rs:0-47
  ...
```

### `git-semantic get <file> [--mode outline|signatures|full]`

Retrieve a file by path or a specific chunk by line range.

**File-level retrieval** (three modes powered by tree-sitter):

```bash
git-semantic get src/db.rs --mode outline      # name + line range per chunk — cheapest
git-semantic get src/db.rs --mode signatures   # full declaration, no body
git-semantic get src/db.rs                     # full content of all chunks
```

Output includes callers — files outside this one that reference it via edges:

```
// src/db.rs
// callers:
//   src/main.rs (via hydrate_from_branch, grep_semantic)

  L1-126    init_with_dimension
  L128-140  clear
  L142-161  insert_subsystem
  L463-497  search_hybrid
```

**Chunk-level retrieval** (exact or overlapping range):

```bash
git-semantic get src/embed.rs:9-17
git-semantic get src/embeddings/config.rs:0-100   # returns all overlapping chunks merged
```

| mode | mechanism | typical savings vs raw |
|------|-----------|----------------------|
| `outline` | tree-sitter extracts identifier name only | ~96% |
| `signatures` | tree-sitter cuts at body node, keeps full declaration | ~86% |
| `full` (default) | all chunks concatenated | ~4% |

### `git-semantic grep <query>`

Search code using three-signal hybrid search: BM25 (FTS5) + semantic embeddings + graph proximity, merged via Reciprocal Rank Fusion. Files connected via call edges to top semantic results are boosted automatically. Higher score = more relevant; a result scoring 2x the next is an unambiguous match.

```bash
git-semantic grep "how incoming requests are validated"
git-semantic grep "error propagation across async boundaries" -n 5
git-semantic grep "ExactIdentifierName"
```

### `git-semantic health [--community <name>]`

Show a cohesion/coupling heatmap of all semantic communities. Use `--community` to drill into a specific one — shows files, top dependents, and top dependencies.

```bash
git-semantic health
git-semantic health --community "database"
```

### `git-semantic benchmark [--json]`

Measure token savings across read modes for every indexed file, and replay actual navigation queries to compare grep vs map+get strategies.

```bash
git-semantic benchmark
git-semantic benchmark --json
```

Output includes:
- Token savings by language (outline / signatures vs raw)
- Read mode comparison table
- Session cost simulation
- Navigation comparison: grep precision vs map+outline+get precision across sampled subsystems

### `git-semantic mcp`

Starts the MCP server (JSON-RPC over stdio). Exposes `map`, `get`, `grep`, and `health` as tools to any MCP-compatible client — Claude Code, Cursor, Codex, Windsurf, and others.

```bash
git-semantic mcp
```

Register it in your client's config:

**Claude Code** (`.claude/settings.json`):
```json
{
  "mcpServers": {
    "git-semantic": {
      "command": "git-semantic",
      "args": ["mcp"]
    }
  }
}
```

**Cursor** (`.cursor/mcp.json`):
```json
{
  "mcpServers": {
    "git-semantic": {
      "command": "git-semantic",
      "args": ["mcp"]
    }
  }
}
```

### `git-semantic usage`

Shows token usage and waste ratio for Claude Code sessions in the current project.

```bash
git-semantic usage           # snapshot
git-semantic usage -w        # watch mode, refreshes every 2s
git-semantic usage -w 5      # watch mode, refreshes every 5s
git-semantic usage -s 10     # show last 10 sessions
```

```
Project: /Users/you/your-project

SESSION        TURNS    BASELINE     LATEST       WASTE      TOTAL      GROWTH
--------------------------------------------------------------------------------
3b218a3a       20       18k          19k          1.1x       374k       ▃▃▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄
```

- **BASELINE** — avg tokens/turn for first 5 turns
- **LATEST** — avg tokens/turn for last 3 turns
- **WASTE** — LATEST / BASELINE (1x = healthy, 5x+ = degrading)
- **GROWTH** — sparkline per turn, yellow at 2.5x+, red at 5x+

### `git-semantic config`

Configure the embedding provider. Stored in `.git/config`, per-repository.

```bash
git-semantic config --list
git-semantic config provider openai
git-semantic config provider gemma
```

---

## Navigation workflow

Once registered as an MCP server, any client can call the tools directly. The intended workflow:

**Step 1 — orient**
```bash
git-semantic map "natural language description of the task"
```
Read the output. If it names the function or file needed — go to step 2 immediately.

**Step 2 — read cheaply**
```bash
git-semantic get src/file.rs --mode outline      # names + line ranges, ~96% token reduction
git-semantic get src/file.rs --mode signatures   # declarations only, ~86% token reduction
```
Start with outline. If the declaration alone is enough, stop. If you need the body, go to step 3.

**Step 3 — retrieve exactly**
```bash
git-semantic get src/file.rs:start-end
```
Use the line ranges from the outline output directly. Maximum 3 calls per task.

**Step 4 — search (last resort)**
```bash
git-semantic grep "natural language query"
git-semantic grep "ExactIdentifierName"
```
Use when the map was genuinely insufficient. Search is hybrid (BM25 + semantic + graph proximity). For exact identifier lookups prefer `grep` over `map` — BM25 will find it precisely.

Orient once, read cheaply, retrieve exactly, never re-search what the map already answered.

---

## Sharing embeddings

Indexing only needs to happen once. Push the `semantic` branch and the whole team benefits — no API keys, no re-embedding.

```bash
# Once, by whoever has an API key
git-semantic index
git push origin semantic

# Everyone else
git fetch origin semantic
git-semantic hydrate
```

### Automated via GitHub Actions

```yaml
name: Semantic Index

on:
  push:
    branches: [main]

jobs:
  index:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
          token: ${{ secrets.GITHUB_TOKEN }}

      - name: Install git-semantic
        run: cargo install gitsem

      - name: Index codebase
        env:
          OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
        run: git-semantic index

      - name: Push semantic branch
        run: |
          git config user.name "github-actions[bot]"
          git config user.email "github-actions[bot]@users.noreply.github.com"
          git push origin semantic
```

---

## Configuration

### Gemma embeddings (default, no API key required)

```bash
git-semantic config provider gemma
```

Model files are cached at `~/.cache/fastembed` by default. Override with `FASTEMBED_CACHE_DIR`.

### OpenAI embeddings

```bash
export OPENAI_API_KEY="sk-..."
git-semantic config provider openai
```

### Available keys

| Key | Default | Description |
|-----|---------|-------------|
| `provider` | `gemma` | Embedding provider: `gemma` or `openai` |
| `openai.model` | `text-embedding-3-small` | OpenAI model |
| `gemma.embeddingDim` | `768` | Gemma embedding dimension |

---

## Supported languages

Rust, Python, JavaScript, TypeScript, Java, C, C++, Go

---

## Project structure

```
git-semantic/
├── src/
│   ├── main.rs              # CLI and command handlers
│   ├── map.rs               # Subsystem and edge data types
│   ├── clustering.rs        # Leiden community detection and edge extraction
│   ├── models.rs            # CodeChunk data structure
│   ├── db.rs                # SQLite + sqlite-vec + FTS5 hybrid search index
│   ├── embed.rs             # Embedding dispatch
│   ├── semantic_branch.rs   # Orphan branch read/write via git worktree
│   ├── embeddings/          # OpenAI, ONNX, and Gemma provider implementations
│   └── chunking/            # tree-sitter parsing and language detection
└── Cargo.toml
```



## License

MIT OR Apache-2.0
