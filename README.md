# git-semantic

Semantic search for your codebase. Parses every tracked file with tree-sitter, generates vector embeddings per chunk, and stores them on a dedicated orphan Git branch that mirrors your source tree — so the whole team can share embeddings without re-indexing.

## How It Works

```
main branch                   semantic branch (orphan)
──────────────────            ──────────────────────────────
src/main.rs          →        src/main.rs       ← [{start_line, end_line, text, embedding}, ...]
src/db.rs            →        src/db.rs         ← [{...}, ...]
src/chunking/mod.rs  →        src/chunking/mod.rs
```

1. `git-semantic index` parses all tracked files with tree-sitter, embeds each chunk, and commits the mirrored JSON files to the `semantic` orphan branch. On subsequent runs it only re-embeds files that changed since the last index (incremental)
2. `git push origin semantic` shares the embeddings with the team
3. Contributors run `git fetch origin semantic` + `git-semantic hydrate` to populate their local SQLite search index — no re-embedding needed
4. `git-semantic grep` runs KNN vector similarity search against the local index

## Sharing Embeddings

Indexing only needs to happen once — whoever runs it pushes the `semantic` branch and the whole team benefits. Nobody else needs an API key or has to re-embed anything.

### Manual

```bash
# Anyone with an API key runs this once (or after significant changes)
git-semantic index
git push origin semantic

# Everyone else
git fetch origin semantic
git-semantic hydrate
git-semantic grep "..."
```

### Automated (GitHub Actions)

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

## Installation

```bash
cargo install gitsem
```

**Prerequisites:** Rust 1.65+, Git 2.0+

## Commands

### `git-semantic index`

Parses and embeds files, then commits the result to the `semantic` orphan branch.

- **First run:** full index of all tracked files
- **Subsequent runs:** incremental — re-embeds only added, modified, renamed, or deleted files
- Respects `.gitignore` (uses `git ls-files`)
- Skips binary files
- Creates the `semantic` branch automatically on first run

### `git-semantic hydrate`

Reads the `semantic` branch and populates the local `.git/semantic.db` search index. Attempts to fetch `origin/semantic` first, then falls back to the local branch.

### `git-semantic grep <query>`

Search code semantically using natural language. Results include the full matched chunk — no file reading needed.

```bash
git-semantic grep "how incoming requests are validated"
git-semantic grep "error propagation across async boundaries" -n 5
```

### `git-semantic enable claude`

Sets up the project for use with Claude Code — injects `CLAUDE.md` instructions, installs hooks that block grep/rg/whole-file reads, and adds token usage monitoring.

```bash
git-semantic enable claude
```

- Writes hook scripts to `.claude/hooks/`
- Wires hooks into `.claude/settings.json`
- Injects search instructions into `CLAUDE.md`
- Idempotent — safe to run multiple times

### `git-semantic usage`

Shows token usage for the current project's Claude Code sessions.

```bash
git-semantic usage
git-semantic usage -s 10
```

```
Project: /Users/you/your-project

SESSION        TURNS    BASELINE     LATEST       WASTE      TOTAL
-----------------------------------------------------------------
3b218a3a       42       19k          24k          1.3x       891k
```

- **BASELINE** — avg tokens/turn for the first 5 turns
- **LATEST** — avg tokens/turn for the last 3 turns
- **WASTE** — LATEST / BASELINE (1x = healthy, 5x+ = degrading, 10x+ = start fresh)
- **TOTAL** — total tokens consumed in the session

### `git-semantic config`

Configure the embedding provider. Config is stored in `.git/config` and is per-repository.

```bash
git-semantic config --list
git-semantic config provider openai
git-semantic config provider onnx
git-semantic config --get provider
git-semantic config --unset onnx.modelPath
```

## Configuration

### OpenAI embeddings

```bash
export OPENAI_API_KEY="sk-..."
git-semantic config provider openai
```

### Local ONNX embeddings

```bash
git-semantic config provider onnx
git-semantic config onnx.modelPath /path/to/model.onnx
```

### Available keys

| Key | Default | Description |
|-----|---------|-------------|
| `provider` | `onnx` | Embedding provider: `openai` or `onnx` |
| `openai.model` | `text-embedding-3-small` | OpenAI model to use |
| `onnx.modelPath` | — | Path to local ONNX model file |
| `onnx.modelName` | `bge-small-en-v1.5` | ONNX model name |

## Supported Languages

Rust, Python, JavaScript, TypeScript, Java, C, C++, Go

## Project Structure

```
git-semantic/
├── src/
│   ├── main.rs              # CLI and command handlers
│   ├── models.rs            # CodeChunk data structure
│   ├── db.rs                # SQLite + sqlite-vec search index
│   ├── embed.rs             # Embedding generation
│   ├── semantic_branch.rs   # Orphan branch read/write via git worktree
│   ├── embeddings/          # OpenAI and ONNX provider implementations
│   └── chunking/            # tree-sitter parsing and language detection
├── Cargo.toml
└── README.md
```

## License

MIT OR Apache-2.0
