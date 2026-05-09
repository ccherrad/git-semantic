# git-semantic

> Semantic search and spatial navigation for Git repositories — so AI coding agents stop searching and start knowing.

`git-semantic` parses every tracked file with tree-sitter, generates vector embeddings per chunk, and stores them on a dedicated orphan Git branch. At index time it also builds a **spatial map** of the codebase — grouping files into semantically coherent subsystems using Leiden community detection, labeling them by their key functions, and tracking cross-file call edges.

Search is hybrid: BM25 (SQLite FTS5) + semantic embeddings merged via Reciprocal Rank Fusion. Exact identifier lookups score higher when they appear in both ranked lists; natural language queries fall back gracefully to semantic-only.

The result: an AI coding agent can orient in one turn instead of five, retrieve exactly what it needs instead of everything that matches, and stay efficient across a long session instead of degrading into context bloat.

---

## The problem

AI coding agents default to exploration. They read files, run searches, accumulate context. By the time they understand the codebase well enough to work, they have already paid for that understanding in tokens — and will keep paying for it on every subsequent turn.

We measure this with the **waste ratio**:

```
waste ratio = avg tokens/turn (last 3 turns) ÷ avg tokens/turn (first 5 turns)
```

1.0x means the session is flat. 2.5x means late turns cost two and a half times the first. Without intervention, agents routinely hit 3-5x on long sessions. The cause is not bad search — it is orientation cost. Every session starts cold and re-discovers the same codebase from scratch.

Semantic search alone does not fix this. It improves retrieval quality but does nothing about the turns spent figuring out what to retrieve. The map fixes orientation.

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

### `git-semantic enable claude`

Installs a Claude Code subagent at `.claude/agents/git-semantic.md`.

- The agent auto-invokes when Claude needs to navigate or search code
- Can also be called explicitly with `@git-semantic`
- No hooks, no blocked tools — the agent opts into the workflow only when relevant
- Idempotent — safe to run multiple times

```bash
git-semantic enable claude
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
git-semantic config provider onnx
git-semantic config provider gemma
```

---

## Agent workflow

After running `git-semantic enable claude`, Claude Code gains a `@git-semantic` subagent. Call it explicitly or let Claude invoke it automatically when it needs to navigate code.

The agent follows this workflow:

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

Outside the agent, normal tools work freely. The navigation discipline applies only when the agent is active — orient once, read cheaply, retrieve exactly, never re-search what the map already answered.

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

### Local ONNX embeddings (no API key required)

```bash
git-semantic config provider onnx
git-semantic config onnx.modelPath /path/to/model.onnx
```

### Available keys

| Key | Default | Description |
|-----|---------|-------------|
| `provider` | `gemma` | Embedding provider: `gemma`, `openai`, or `onnx` |
| `openai.model` | `text-embedding-3-small` | OpenAI model |
| `onnx.modelPath` | — | Path to local ONNX model file |
| `onnx.modelName` | `bge-small-en-v1.5` | ONNX model name |
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
