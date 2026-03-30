# git-semantic

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

### Build from Source

```bash
git clone https://github.com/yourusername/git-semantic.git
cd git-semantic
cargo install --path .
```

The binary will be installed to `~/.cargo/bin/git-semantic`.

### Verify Installation

```bash
git semantic help
```

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
         │ git semantic commit/reindex
         ▼
┌─────────────────────────────────┐
│   Git Notes (refs/notes/semantic)│  ← Source of Truth
│   - Commit metadata              │
│   - Diffs                        │
│   - Vector embeddings (768-dim)  │
└────────┬────────────────────────┘
         │ git semantic pull
         ▼
┌─────────────────┐
│  SQLite (.git/  │  ← Search Index
│  semantic.db)   │
│  - vec0 virtual │
│    table        │
└─────────────────┘
         │
         ▼ git semantic grep
┌─────────────────┐
│  Vector Search  │
│  Results        │
└─────────────────┘
```

### Data Flow

1. **Create Semantic Notes**: `git semantic commit` or `reindex` generates embeddings and stores them as Git notes
2. **Sync Across Team**: `git push origin refs/notes/semantic` shares notes with teammates
3. **Build Search Index**: `git semantic pull` fetches notes and populates local SQLite database
4. **Search**: `git semantic grep` performs KNN vector similarity search

## Commands

### `git semantic commit`

Create a commit with semantic notes attached.

```bash
# Commit with all changes
git semantic commit -a -m "Add user authentication"

# Commit staged changes
git add .
git semantic commit -m "Fix login bug"

# Interactive (prompts for message)
git semantic commit
```

**What it does:**
- Creates a regular Git commit
- Generates embeddings from the diff
- Attaches semantic note to the commit in `refs/notes/semantic`

### `git semantic reindex <range>`

Add semantic notes to existing commits retroactively.

```bash
# Index last 3 commits
git semantic reindex HEAD~3..HEAD

# Index all commits since main
git semantic reindex main..HEAD

# Index specific range
git semantic reindex abc123..def456
```

**What it does:**
- Fetches all commits in the range
- Generates embeddings for each commit's diff
- Attaches semantic notes to existing commits

### `git semantic pull [remote]`

Pull code changes and sync semantic notes.

```bash
# Pull from origin (default)
git semantic pull

# Pull from upstream
git semantic pull upstream
```

**What it does:**
- Executes `git pull`
- Fetches `refs/notes/semantic` from remote
- Rebuilds local SQLite database from notes

### `git semantic grep <query>`

Search code semantically using natural language.

```bash
# Basic search
git semantic grep "authentication logic"

# Limit results
git semantic grep "error handling" -n 5
```

**What it does:**
- Generates embedding for the query
- Performs KNN vector similarity search
- Returns semantically similar code chunks

### `git semantic show [commit]`

View semantic note attached to a commit.

```bash
# Show note for HEAD
git semantic show

# Show note for specific commit
git semantic show abc123

# Show note for HEAD~2
git semantic show HEAD~2
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

# Index the last 10 commits
git semantic reindex HEAD~10..HEAD

# Share semantic notes with team
git push origin refs/notes/semantic
```

### Example 2: Daily Development Workflow

```bash
# Make changes
vim src/auth.rs

# Create commit with semantic notes
git semantic commit -a -m "feat: add JWT token validation"

# Pull teammate's changes and sync semantics
git semantic pull

# Search for related code
git semantic grep "token validation logic"
```

### Example 3: Code Review

```bash
# View semantic context of a commit
git semantic show HEAD~2

# Search for similar patterns
git semantic grep "similar authentication pattern"
```

### Example 4: Team Collaboration

```bash
# Developer A: Create semantic commits
git semantic commit -m "refactor: simplify error handling"
git push origin main refs/notes/semantic

# Developer B: Pull and sync
git semantic pull
git semantic grep "error handling patterns"
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
git-semantic/
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
