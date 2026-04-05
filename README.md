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

1. `git-semantic index` parses all tracked files with tree-sitter, embeds each chunk, and commits the mirrored JSON files to the `semantic` orphan branch
2. `git push origin semantic` shares the embeddings with the team
3. Contributors run `git fetch origin semantic` + `git-semantic hydrate` to populate their local SQLite search index — no re-embedding needed
4. `git-semantic grep` runs KNN vector similarity search against the local index

## Indexing Strategies

Two strategies are supported and can be used independently or together.

### Strategy 1: Local Indexing

A developer (or maintainer) runs indexing on their machine and pushes the semantic branch. Teammates pull and hydrate — no API key or re-embedding needed on their end.

```
┌─────────────────────────────────────────────────────┐
│  Maintainer / designated indexer                    │
│                                                     │
│  git-semantic index   ← embeds all files locally   │
│  git push origin semantic                           │
└────────────────────────┬────────────────────────────┘
                         │
          ┌──────────────▼──────────────┐
          │       origin/semantic       │
          └──────────────┬──────────────┘
                         │
        ┌────────────────▼────────────────┐
        │  Any contributor                │
        │                                 │
        │  git fetch origin semantic      │
        │  git-semantic hydrate           │
        │  git-semantic grep "..."        │
        └─────────────────────────────────┘
```

**When to use:** Small teams, cost-sensitive setups, or when you prefer full control over when embeddings are regenerated.

### Strategy 2: CI/CD Continuous Indexing

The CI pipeline runs `git-semantic index` automatically on every push to the main branch and pushes the updated semantic branch. The team always has fresh embeddings without anyone having to run indexing manually.

```
┌─────────────────────────────────────────────────────┐
│  Developer pushes to main                           │
└────────────────────────┬────────────────────────────┘
                         │
          ┌──────────────▼──────────────┐
          │  CI pipeline (GitHub        │
          │  Actions / GitLab CI / etc) │
          │                             │
          │  git-semantic index         │
          │  git push origin semantic   │
          └──────────────┬──────────────┘
                         │
          ┌──────────────▼──────────────┐
          │       origin/semantic       │
          └──────────────┬──────────────┘
                         │
        ┌────────────────▼────────────────┐
        │  Any contributor                │
        │                                 │
        │  git fetch origin semantic      │
        │  git-semantic hydrate           │
        │  git-semantic grep "..."        │
        └─────────────────────────────────┘
```

**When to use:** Teams that want always-fresh embeddings with zero manual steps. Requires an API key configured as a CI secret (for OpenAI) or a bundled ONNX model.

### Comparing the two strategies

| | Local | CI/CD |
|---|---|---|
| Who runs indexing | Designated person | Automated on push |
| Embedding freshness | Manual, on demand | Automatic on every merge |
| API key exposure | Developer's machine only | CI secret |
| Setup complexity | None | CI workflow + secret |
| Cost control | Full | Per-push (use incremental indexing) |

## Installation

### Prerequisites

- Rust 1.65 or higher
- Git 2.0 or higher

### From crates.io

```bash
cargo install git-semantic
```

### Build from Source

```bash
git clone https://github.com/ccherrad/git-semantic.git
cd git-semantic
cargo install --path .
```

## Workflow

### Local Strategy

```bash
# Run once (or when you want to refresh embeddings)
git-semantic index
git push origin semantic

# Contributors
git fetch origin semantic
git-semantic hydrate
git-semantic grep "authentication middleware"
```

### CI/CD Strategy (GitHub Actions)

Add `.github/workflows/semantic-index.yml` to your repository:

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
        run: cargo install git-semantic

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

Contributors still just run:

```bash
git fetch origin semantic
git-semantic hydrate
git-semantic grep "..."
```

## Commands

### `git-semantic index`

Parses and embeds all files tracked by git, then commits the result to the `semantic` orphan branch.

```bash
git-semantic index
```

- Respects `.gitignore` (uses `git ls-files`)
- Skips binary files
- Files with unrecognized extensions are stored as a single chunk
- Creates the `semantic` branch automatically on first run

### `git-semantic hydrate`

Reads the `semantic` branch and populates the local `.git/semantic.db` search index.

```bash
git-semantic hydrate
```

Attempts to fetch `origin/semantic` first, then falls back to the local branch.

### `git-semantic grep <query>`

Search code semantically using natural language.

```bash
git-semantic grep "authentication logic"
git-semantic grep "error handling" -n 5
```

### `git-semantic config`

Configure the embedding provider.

```bash
git-semantic config --list
git-semantic config gitsem.provider openai
git-semantic config gitsem.provider onnx
git-semantic config --get gitsem.provider
git-semantic config --unset gitsem.onnx.modelPath
```

## Configuration

### OpenAI embeddings

```bash
export OPENAI_API_KEY="sk-..."
git-semantic config gitsem.provider openai
```

### Local ONNX embeddings

```bash
git-semantic config gitsem.provider onnx
git-semantic config gitsem.onnx.modelPath /path/to/model.onnx
```

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

## Building

```bash
cargo build --release
cargo test
```

## License

MIT OR Apache-2.0
