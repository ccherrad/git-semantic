# gitsem

A semantic search layer for Git repositories that augments commits with vector embeddings, enabling AI agents and developers to search code by meaning rather than text patterns.

## Features

- **Semantic Commit Notes**: Automatically attach embeddings and context to commits
- **Vector Search**: Search code using natural language queries
- **Git-Native**: Uses Git notes (`refs/notes/semantic`) for storage
- **Team Collaboration**: Share semantic indexes via git push/pull
- **Retroactive Indexing**: Add semantic notes to existing commit history
- **Idempotent Operations**: Safe to run after regular git commands

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

The binary will be installed to `~/.cargo/bin/gitsem`.

### Verify Installation

```bash
gitsem help
# OR use as git subcommand
git semantic help
```

Both `gitsem` and `git-semantic` binaries are installed, so you can use either:
- `gitsem <command>` - Standalone command
- `git semantic <command>` - Git subcommand style

If the command isn't found, ensure `~/.cargo/bin` is in your PATH:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Add this to your `~/.bashrc` or `~/.zshrc` to make it permanent.

## How It Works

### Architecture

```
┌─────────────────┐
│   Git Commits   │
└────────┬────────┘
         │ gitsem commit/reindex
         ▼
┌─────────────────────────────────┐
│   Git Notes (refs/notes/semantic)│  ← Source of Truth
│   - Commit metadata              │
│   - Diffs                        │
│   - Vector embeddings (768-dim)  │
└────────┬────────────────────────┘
         │ gitsem pull
         ▼
┌─────────────────┐
│  SQLite (.git/  │  ← Search Index
│  semantic.db)   │
│  - vec0 virtual │
│    table        │
└─────────────────┘
         │
         ▼ gitsem grep
┌─────────────────┐
│  Vector Search  │
│  Results        │
└─────────────────┘
```

### Data Flow

1. **Create Semantic Notes**: `gitsem commit` or `reindex` generates embeddings and stores them as Git notes
2. **Sync Across Team**: `git push origin refs/notes/semantic` shares notes with teammates
3. **Build Search Index**: `gitsem pull` fetches notes and populates local SQLite database
4. **Search**: `gitsem grep` performs KNN vector similarity search

## Commands

### `gitsem commit`

Create a commit with semantic notes attached.

```bash
# Commit with all changes
gitsem commit -a -m "Add user authentication"

# Commit staged changes
git add .
gitsem commit -m "Fix login bug"

# Interactive (prompts for message)
gitsem commit
```

**What it does:**
- Creates a regular Git commit
- Generates embeddings from the diff
- Attaches semantic note to the commit in `refs/notes/semantic`

### `gitsem reindex <range>`

Add semantic notes to existing commits retroactively.

```bash
# Index last 3 commits
gitsem reindex HEAD~3..HEAD

# Index all commits since main
gitsem reindex main..HEAD

# Index specific range
gitsem reindex abc123..def456
```

**What it does:**
- Fetches all commits in the range
- Generates embeddings for each commit's diff
- Attaches semantic notes to existing commits

### `gitsem pull [remote]`

Pull code changes and sync semantic notes.

```bash
# Pull from origin (default)
gitsem pull

# Pull from upstream
gitsem pull upstream
```

**What it does:**
- Executes `git pull`
- Fetches `refs/notes/semantic` from remote
- Rebuilds local SQLite database from notes

### `gitsem grep <query>`

Search code semantically using natural language.

```bash
# Basic search
gitsem grep "authentication logic"

# Limit results
gitsem grep "error handling" -n 5
```

**What it does:**
- Generates embedding for the query
- Performs KNN vector similarity search
- Returns semantically similar code chunks

### `gitsem show [commit]`

View semantic note attached to a commit.

```bash
# Show note for HEAD
gitsem show

# Show note for specific commit
gitsem show abc123

# Show note for HEAD~2
gitsem show HEAD~2
```

**What it does:**
- Displays formatted semantic note
- Shows embedding dimensions
- Previews commit content and diff

## Examples

### Example 1: New Repository Setup

```bash
# Clone a repository
git clone https://github.com/example/myproject.git
cd myproject

# Index the last 10 commits (use either style)
gitsem reindex HEAD~10..HEAD
# OR: git semantic reindex HEAD~10..HEAD

# Share semantic notes with team
git push origin refs/notes/semantic
```

### Example 2: Daily Development Workflow

```bash
# Make changes
vim src/auth.rs

# Create commit with semantic notes
gitsem commit -a -m "feat: add JWT token validation"
# OR: git semantic commit -a -m "feat: add JWT token validation"

# Pull teammate's changes and sync semantics
gitsem pull
# OR: git semantic pull

# Search for related code
gitsem grep "token validation logic"
# OR: git semantic grep "token validation logic"
```

### Example 3: Code Review

```bash
# View semantic context of a commit
gitsem show HEAD~2

# Search for similar patterns
gitsem grep "similar authentication pattern"
```

### Example 4: Team Collaboration

```bash
# Developer A: Create semantic commits
gitsem commit -m "refactor: simplify error handling"
git push origin main refs/notes/semantic

# Developer B: Pull and sync
gitsem pull
gitsem grep "error handling patterns"
```

## Configuration

### Environment Variables

**OPENAI_API_KEY** (Required for real embeddings)

Currently, the embedding generator is a placeholder. To use real embeddings:

1. Set your OpenAI API key:
   ```bash
   export OPENAI_API_KEY="sk-..."
   ```

2. Add to your shell config (`~/.bashrc` or `~/.zshrc`):
   ```bash
   export OPENAI_API_KEY="sk-..."
   ```

### Git Configuration

Semantic notes are stored in `refs/notes/semantic`. To automatically fetch notes:

```bash
git config --add remote.origin.fetch "+refs/notes/semantic:refs/notes/semantic"
```

## Current Limitations

1. **Placeholder Embeddings**: The current implementation uses dummy embeddings (768-dimensional vectors with sequential values). Real LLM API integration (OpenAI, Cohere, etc.) needs to be implemented in `src/embed.rs`.

2. **SQLite-vec Integration**: The `vec0` virtual table is defined but requires the sqlite-vec extension to be loaded at runtime for production vector search.

3. **No Automatic Sync**: Semantic notes must be manually pushed/pulled via `git push origin refs/notes/semantic`.

## Development

### Project Structure

```
gitsem/
├── src/
│   ├── main.rs       # CLI and command handlers
│   ├── models.rs     # CodeChunk data structure
│   ├── db.rs         # SQLite database with vec0 table
│   ├── git.rs        # Git notes read/write operations
│   └── embed.rs      # Embedding generation (placeholder)
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

### Installing Locally

```bash
cargo install --path .
```

## Roadmap

- [ ] Real embedding API integration (OpenAI, Cohere, local models)
- [ ] Load sqlite-vec extension for production vector search
- [ ] Automatic note syncing on push/pull
- [ ] Support for multiple embedding models
- [ ] Web UI for browsing semantic history
- [ ] VS Code extension
- [ ] GitHub Action for CI/CD integration

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Submit a pull request

## License

MIT OR Apache-2.0

## Acknowledgments

- Built with [gix](https://github.com/GitoxideLabs/gitoxide) - Pure Rust Git implementation
- Inspired by the need for semantic code search in AI-assisted development
