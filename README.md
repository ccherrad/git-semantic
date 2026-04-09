# git-semantic

> Semantic search for Git repositories — and a study case on whether semantic search actually makes AI coding agents better.

Semantic search for your codebase. Parses every tracked file with tree-sitter, generates vector embeddings per chunk, and stores them on a dedicated orphan Git branch — so the whole team can share embeddings without re-indexing.

But this project is also an experiment: **does replacing grep with semantic search inside an AI coding agent produce better results, and at what cost?** The benchmark results and methodology are documented below.

---

## The Problem

AI coding agents default to grep. They search by keywords, retrieve whole files, and accumulate everything into context. The result is agents that are expensive, noisy, and increasingly slow as sessions grow.

The hypothesis: if you replace keyword search with semantic search — retrieving the most relevant chunk instead of every line matching a string — agents should find better answers with less context overhead.

But hypotheses need evidence.

---

## The Experiment

### Setup

I ran two Claude agents against the same codebase — this one — with 20 deep investigation tasks each. Tasks covered distinct subsystems: embedding provider dispatch, incremental indexing, vector serialization, error propagation, CLI routing, language detection, and more.

- **Agent A (`with-embedding`)** — `git-semantic grep` enforced via a `PreToolUse` hook that blocked `grep`, `rg`, and the built-in Grep tool. Whole-file reads were also blocked — the agent had to find the chunk first.
- **Agent B (`without-embedding`)** — plain `grep`/`rg`, no restrictions.

Results were judged blind on accuracy, depth, code precision, notable findings, and format.

### Quality Results

| Dimension | Semantic agent | Grep agent |
|---|---|---|
| Accuracy | 9/10 | 9/10 |
| Depth of explanation | 9/10 | 8/10 |
| Code precision | 8/10 | 9/10 |
| Notable findings | 10/10 | 8/10 |
| Format consistency | 10/10 | 9/10 |
| **Total** | **46/50** | **43/50** |

The semantic agent surfaced findings the grep agent missed or softened:

- **A silent dimension mismatch bug** — switching embedding models after indexing silently breaks the SQLite schema with no migration path and no clear error.
- **A UTF-8 truncation issue** — the OpenAI provider truncates text by byte length, which can split multi-byte characters mid-codepoint.
- **A batching gap** — the OpenAI embeddings API supports 2048 inputs per request; the implementation sends one per HTTP call, making large repo indexing unnecessarily slow.

### Efficiency Results

| | Semantic agent | Grep agent |
|---|---|---|
| Total tokens | 484k | 713k |
| Total turns | 19 | 25 |
| Token savings | — | +47% more |
| Turn savings | — | +32% more |

**32% fewer tokens. 24% fewer turns.** Targeted chunk retrieval meant less noise in context and fewer follow-up reads.

### The Waste Ratio

Total token count is not the most important metric. The more revealing number is how token usage *per turn* changes over the course of a session.

> **waste ratio = avg tokens/turn (last 3 turns) ÷ avg tokens/turn (first 5 turns)**

A ratio of **1x** means the agent is as efficient at turn 20 as turn 1. In practice, agents carry their entire conversation history forward — completed task results, intermediate reasoning, redundant reads — and cost compounds.

| | Baseline (first 5 turns) | Latest (last 3 turns) | Waste ratio |
|---|---|---|---|
| Semantic agent | ~17k tokens/turn | ~43k tokens/turn | **2.5x** |
| Grep agent | ~18k tokens/turn | ~49k tokens/turn | **2.7x** |

Both agents degraded significantly. Semantic search improved retrieval. It did not fix context bloat. By the end of 20 tasks, both agents were paying for history, not output.

### Observation: Instructions Don't Enforce Behavior

Before introducing the hook-based enforcement, a `CLAUDE.md` rule told agents to use `git-semantic grep`. The main session followed it. Subagents ignored it and ran `rg` anyway — silently.

Instructions express intent. Hooks enforce behavior. The `PreToolUse` hook that blocks grep/rg/Grep is what actually changed agent behavior — not the instruction.

---

## What This Suggests

Semantic search wins on both quality and cost. But the waste ratio finding points at a deeper problem: **context engineering matters as much as search quality.**

How you shape the information you give an agent — the size of it, the timing of it, what you withhold until it's needed — is the real variable. Search is one input. It's not the whole answer.

Two directions worth exploring:

**Directory-level context.** When an agent decomposes a task and routes a subtask to "the database layer", it currently has no spatial awareness — it reads everything. If directories carried semantic descriptions, the agent could load only the context hierarchy relevant to that scope. Narrower search space, smaller context, less noise. Same principle as chunk-level semantic search, one level up.

**Session decomposition.** Instead of one long session for many tasks, split into smaller sessions with structured handoff summaries. Each session starts fresh. The handoff carries only conclusions, not reasoning chains. This trades some continuity for a much lower waste floor.

---

## How It Works

```
main branch                   semantic branch (orphan)
──────────────────            ──────────────────────────────
src/main.rs          →        src/main.rs       ← [{start_line, end_line, text, embedding}, ...]
src/db.rs            →        src/db.rs         ← [{...}, ...]
src/chunking/mod.rs  →        src/chunking/mod.rs
```

1. `git-semantic index` parses all tracked files with tree-sitter, embeds each chunk, and commits the mirrored JSON files to the `semantic` orphan branch. Subsequent runs re-embed only changed files.
2. `git push origin semantic` shares the embeddings with the team.
3. Contributors run `git fetch origin semantic` + `git-semantic hydrate` to populate their local SQLite search index — no re-embedding needed.
4. `git-semantic grep` runs KNN vector similarity search against the local index.

---

## Installation

```bash
cargo install gitsem
```

**Prerequisites:** Rust 1.65+, Git 2.0+

---

## Commands

### `git-semantic index`

Parses and embeds files, then commits the result to the `semantic` orphan branch.

- First run: full index of all tracked files
- Subsequent runs: incremental — re-embeds only added, modified, renamed, or deleted files
- Respects `.gitignore` (uses `git ls-files`)
- Skips binary files

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

Shows token usage and waste ratio for the current project's Claude Code sessions.

```bash
git-semantic usage
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
```

---

## Sharing Embeddings

Indexing only needs to happen once. Whoever runs it pushes the `semantic` branch and the whole team benefits. Nobody else needs an API key or has to re-embed anything.

```bash
# Anyone with an API key runs this once
git-semantic index
git push origin semantic

# Everyone else
git fetch origin semantic
git-semantic hydrate
git-semantic grep "..."
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

---

## Supported Languages

Rust, Python, JavaScript, TypeScript, Java, C, C++, Go

---

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
├── BENCHMARK_TASKS.md       # The 20 investigation tasks used in the benchmark
├── Cargo.toml
└── README.md
```

---

## Technology Radar Alignment

`git-semantic` is positioned at the intersection of several techniques on the [ThoughtWorks Technology Radar](https://www.thoughtworks.com/radar). Here is where the project stands against each relevant blip — what is implemented today and what is coming.

---

### [Curated Shared Instructions for Software Teams](https://www.thoughtworks.com/radar/techniques/curated-shared-instructions-for-software-teams) — Adopt

> Committing team-wide reusable instructions (AGENTS.md, CLAUDE.md) to project repositories so that when a prompt is refined, the entire team benefits immediately.

**Status: implemented.** `git-semantic enable claude` injects a `CLAUDE.md` block with mandatory search instructions into the project repository. The instructions are versioned alongside the code, shared via git, and idempotent — safe to run multiple times. Every agent that opens the repo inherits the same search behavior without manual setup.

---

### [Context Engineering](https://www.thoughtworks.com/radar/techniques/context-engineering) — Assess

> Systematically designing and optimizing the information provided to LLMs during inference — structuring prompts, retrieved data, memory, and environmental signals to improve output quality and reduce token overhead.

**Status: partially implemented, actively explored.** The hook system (`PreToolUse` blocking grep/rg/whole-file reads) is a form of context control — it forces the agent to retrieve targeted chunks instead of accumulating full files. The `git-semantic usage` waste ratio metric is a direct measure of context engineering health. Directory-level semantic context (loading only the relevant scope when a task is decomposed) is the next step and is currently being designed.

---

### [On-Device Information Retrieval](https://www.thoughtworks.com/radar/techniques/on-device-information-retrieval) — Assess

> Running search and retrieval-augmented generation entirely on local devices for privacy and efficiency, using sqlite-vec paired with a lightweight local inference model. ThoughtWorks specifically highlights EmbeddingGemma (300M parameters) as a promising option for resource-constrained environments.

**Status: partially implemented, EmbeddingGemma coming soon.** `git-semantic` already uses sqlite-vec as its vector store — the same stack ThoughtWorks recommends. The local ONNX embedding path keeps data fully on-device with no API calls. The current ONNX model (bge-small-en-v1.5) is a placeholder; full ONNX inference and EmbeddingGemma support are on the roadmap as a direct response to this blip.

---

### [Anchoring Coding Agents to a Reference Application](https://www.thoughtworks.com/radar/techniques/anchoring-coding-agents-to-a-reference-application) — Assess

> Providing coding agents with a live, compilable reference application instead of static templates — using an MCP server that exposes both reference code and commit diffs — so agents can reference living blueprints rather than stale documentation.

**Status: coming soon.** The semantic index on the `semantic` branch is already a versioned, queryable representation of the codebase that evolves with commits. Exposing this index via an MCP server — so agents can query "what does the reference implementation look like for X" against a canonical codebase — is a natural next step and is on the roadmap.

---

### [Team of Coding Agents](https://www.thoughtworks.com/radar/techniques/team-of-coding-agents) — Assess

> Coordinating multiple AI agents with assigned roles (architect, backend specialist, tester) working together on development tasks, enabling more sophisticated orchestrated workflows beyond one-to-one agent mappings.

**Status: coming soon.** The current benchmark revealed a key problem: subagents ignore `CLAUDE.md` rules and silently fall back to grep. Solving this is a prerequisite for multi-agent coordination — if you cannot enforce consistent tool behavior across one subagent, you cannot coordinate a team of them. Hook-based enforcement is the foundation. Scoped semantic search per agent role (backend agent searches only backend chunks, etc.) is the next layer.

---

### [Knowledge Flows Over Knowledge Stocks](https://www.thoughtworks.com/radar/techniques/knowledge-flows-over-knowledge-stocks) — Assess

> Prioritizing how knowledge moves through an organization over simply accumulating it — emphasizing communities of practice and active inflow of external knowledge over maintaining static repositories.

**Status: architectural intent, not yet measured.** The `semantic` orphan branch model is designed for flow: one person indexes, the whole team pulls. Embeddings are not a stock that each developer maintains locally — they flow through git. Whether this meaningfully changes knowledge dynamics in a team is not yet measured. It is a hypothesis the project is built on.

---

### [Spec-Driven Development](https://www.thoughtworks.com/radar/techniques/spec-driven-development) — Assess

> Beginning with structured functional specifications that are progressively broken down into smaller components for AI agents, using tools like Amazon's Kiro or GitHub's spec-kit.

**Status: not implemented.** `git-semantic` does not currently generate or consume specs. This is the furthest blip from the current implementation. It is worth watching — if context engineering and semantic search become mature, spec-driven workflows become more tractable. Not on the near-term roadmap.

---

## Reproducing the Benchmark

The 20 tasks are in `BENCHMARK_TASKS.md`. To run them yourself:

```bash
# Agent A — with semantic search enforced
git-semantic enable claude
claude -p "Read BENCHMARK_TASKS.md and follow the instructions in it exactly. Write all results to BENCHMARK_RESULTS_semantic.md"

# Agent B — grep baseline (run in a copy of the repo without the hook setup)
claude -p "Use grep or rg for all code searches. Read BENCHMARK_TASKS.md and follow the instructions in it exactly. Write all results to BENCHMARK_RESULTS_grep.md"
```

Then compare `BENCHMARK_RESULTS_semantic.md` and `BENCHMARK_RESULTS_grep.md`. Use `git-semantic usage` to compare session token consumption and waste ratios.

---

## License

MIT OR Apache-2.0
