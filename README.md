# gitsem

Semantic search for your codebase. Walks all tracked files, chunks them by language using tree-sitter, and builds a local vector index — so you can search code by meaning rather than text patterns.

## Features

- **File-level indexing**: Chunks every tracked file by language using tree-sitter (functions, classes, structs, impls, etc.)
- **Vector Search**: Search code using natural language queries
- **Language-aware**: Supports Rust, Python, JavaScript, TypeScript, Java, C, C++, Go
- **Local index**: SQLite + sqlite-vec stored at `.git/semantic.db`
- **Configurable embeddings**: OpenAI or local ONNX models

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

If not found, add `~/.cargo/bin` to your PATH.

## How It Works

```
git ls-files
     │
     ▼
tree-sitter chunking (per language)
     │
     ▼
embedding (OpenAI / ONNX)
     │
     ▼
SQLite + vec0 (.git/semantic.db)
     │
     ▼ gitsem grep
vector similarity search
```

1. `gitsem index` walks all files tracked by git (respects `.gitignore`)
2. Each file is parsed by tree-sitter and split into top-level constructs
3. Each chunk is embedded and stored in the local SQLite index
4. `gitsem grep` embeds your query and runs KNN search against the index

## Commands

### `gitsem index`

Index all tracked files in the repository.

```bash
gitsem index
```

Clears the existing index and rebuilds it from scratch. Binary files and files with unrecognized extensions are skipped. Unrecognized-language files are stored as a single chunk.

### `gitsem grep <query>`

Search code semantically using natural language.

```bash
gitsem grep "authentication logic"
gitsem grep "error handling" -n 5
```

### `gitsem config`

Get and set embedding provider configuration.

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

## Development

### Project Structure

```
gitsem/
├── src/
│   ├── main.rs          # CLI and command handlers
│   ├── models.rs        # CodeChunk data structure
│   ├── db.rs            # SQLite database with vec0 table
│   ├── embed.rs         # Embedding generation
│   ├── embeddings/      # OpenAI and ONNX providers
│   └── chunking/        # tree-sitter parsing and language detection
├── Cargo.toml
└── README.md
```

### Building

```bash
cargo build --release
```

### Testing

```bash
cargo test
```

## License

MIT OR Apache-2.0
