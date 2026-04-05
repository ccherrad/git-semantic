# gitsem

Semantic search for your codebase. Parses every tracked file with tree-sitter, generates vector embeddings per chunk, and stores them on a dedicated orphan Git branch that mirrors your source tree — so the whole team can share embeddings without re-indexing.

## How It Works

```
main branch                   semantic branch (orphan)
──────────────────            ──────────────────────────────
src/main.rs          →        src/main.rs       ← [{start_line, end_line, text, embedding}, ...]
src/db.rs            →        src/db.rs         ← [{...}, ...]
src/chunking/mod.rs  →        src/chunking/mod.rs
```

1. `gitsem index` parses all tracked files with tree-sitter, embeds each chunk, and commits the mirrored JSON files to the `semantic` orphan branch
2. `git push origin semantic` shares the embeddings with the team
3. Contributors run `git fetch origin semantic` + `gitsem hydrate` to populate their local SQLite search index — no re-embedding needed
4. `gitsem grep` runs KNN vector similarity search against the local index

## Installation

### Prerequisites

- Rust 1.65 or higher
- Git 2.0 or higher

### From crates.io

```bash
cargo install gitsem
```

### Build from Source

```bash
git clone https://github.com/ccherrad/gitsem.git
cd gitsem
cargo install --path .
```

Both `gitsem` and `git-semantic` binaries are installed:
- `gitsem <command>` — standalone
- `git semantic <command>` — git subcommand style

## Workflow

### Maintainer / CI

```bash
gitsem index
git push origin semantic
```

### Contributors

```bash
git fetch origin semantic
gitsem hydrate
gitsem grep "authentication middleware"
```

## Commands

### `gitsem index`

Parses and embeds all files tracked by git, then commits the result to the `semantic` orphan branch.

```bash
gitsem index
```

- Respects `.gitignore` (uses `git ls-files`)
- Skips binary files
- Files with unrecognized extensions are stored as a single chunk
- Creates the `semantic` branch automatically on first run

### `gitsem hydrate`

Reads the `semantic` branch and populates the local `.git/semantic.db` search index.

```bash
gitsem hydrate
```

Attempts to fetch `origin/semantic` first, then falls back to the local branch.

### `gitsem grep <query>`

Search code semantically using natural language.

```bash
gitsem grep "authentication logic"
gitsem grep "error handling" -n 5
```

### `gitsem config`

Configure the embedding provider.

```bash
gitsem config --list
gitsem config gitsem.provider openai
gitsem config gitsem.provider onnx
gitsem config --get gitsem.provider
gitsem config --unset gitsem.onnx.modelPath
```

## Configuration

### OpenAI embeddings

```bash
export OPENAI_API_KEY="sk-..."
gitsem config gitsem.provider openai
```

### Local ONNX embeddings

```bash
gitsem config gitsem.provider onnx
gitsem config gitsem.onnx.modelPath /path/to/model.onnx
```

## Supported Languages

Rust, Python, JavaScript, TypeScript, Java, C, C++, Go

## Project Structure

```
gitsem/
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

## Building

```bash
cargo build --release
cargo test
```

## License

MIT OR Apache-2.0
