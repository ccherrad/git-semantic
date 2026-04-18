# Map Benchmark — Waste Ratio, Orientation Cost, Task Accuracy

Three agents, same 20 tasks, same codebase. Measures the impact of the spatial map on session efficiency and answer quality.

---

## Agents

| Agent | Tools allowed | Setup |
|-------|--------------|-------|
| A — grep | `grep`, `rg`, full file reads | No git-semantic |
| B — semantic | `git-semantic grep` only, no file reads | `git-semantic enable claude` (grep rules only) |
| C — map | `git-semantic map`, `git-semantic get`, `git-semantic grep` (last resort) | `git-semantic enable claude` (full map rules) |

Agents B and C use hook enforcement. Agent A has no hooks.

---

## How to run

### Agent A — grep baseline

Run in a clean copy of the repo with no hook setup:

```bash
cp -r . /tmp/bench-grep && cd /tmp/bench-grep
claude -p "$(cat <<'EOF'
You are investigating the git-semantic Rust codebase (src/).
Use grep or rg for all code searches. Do not use git-semantic.

Read BENCHMARK_MAP_TASKS.md. For each of the 20 tasks:
1. Run the search commands needed to answer it
2. Append your findings to BENCHMARK_RESULTS_A_grep.md
   Each section must start with: ## Task N: <title>
   Include: commands run, file:line references, explanation, notable findings

Work sequentially. Do not skip any task.
EOF
)"
```

### Agent B — semantic grep only

Run in the repo with semantic grep enforced but map disabled:

```bash
# Temporarily inject grep-only CLAUDE.md (no map rules)
cat > CLAUDE.md << 'EOF'
<!-- semantic:agentic-setup -->
## Code Navigation — MANDATORY
NEVER use grep, git grep, rg, ripgrep, or whole-file reads.
ALWAYS use: git-semantic grep "<natural language query>"
Results include the full matched chunk. Do not open files to read functions.
<!-- end semantic:agentic-setup -->
EOF

claude -p "$(cat <<'EOF'
You are investigating the git-semantic Rust codebase (src/).
Follow the CLAUDE.md instructions exactly.

Read BENCHMARK_MAP_TASKS.md. For each of the 20 tasks:
1. Run the search commands needed to answer it
2. Append your findings to BENCHMARK_RESULTS_B_semantic.md
   Each section must start with: ## Task N: <title>
   Include: commands run, file:line references, explanation, notable findings

Work sequentially. Do not skip any task.
EOF
)"
```

### Agent C — map + get

Run in the repo with full map rules enforced:

```bash
git-semantic enable claude  # installs full map rules

claude -p "$(cat <<'EOF'
You are investigating the git-semantic Rust codebase (src/).
Follow the CLAUDE.md instructions exactly.

Read BENCHMARK_MAP_TASKS.md. For each of the 20 tasks:
1. Orient with git-semantic map
2. Retrieve with git-semantic get
3. Use git-semantic grep only if map was insufficient
4. Append your findings to BENCHMARK_RESULTS_C_map.md
   Each section must start with: ## Task N: <title>
   Include: commands run, file:line references, explanation, notable findings

Work sequentially. Do not skip any task.
EOF
)"
```

---

## Measuring waste ratio and orientation cost

After each run:

```bash
git-semantic usage -s 1
```

Record for each agent:
- **BASELINE** — avg tokens/turn for first 5 turns
- **LATEST** — avg tokens/turn for last 3 turns
- **WASTE** — LATEST / BASELINE
- **TOTAL** — total tokens consumed
- **TURNS** — total turns

For orientation cost, inspect the session JSONL directly and count how many turns before the agent writes its first `## Task 1` answer:

```bash
python3 - <<'EOF'
import json, sys

path = sys.argv[1]  # path to .jsonl session file
turn = 0
with open(path) as f:
    for line in f:
        r = json.loads(line)
        if r.get("type") == "assistant":
            turn += 1
            content = r.get("message", {}).get("content", [])
            for c in content:
                if isinstance(c, dict) and c.get("type") == "tool_use":
                    inp = str(c.get("input", {}))
                    if "BENCHMARK_RESULTS" in inp or "Task 1" in inp:
                        print(f"First answer written at turn {turn}")
                        sys.exit(0)
            for c in content:
                if isinstance(c, dict) and c.get("type") == "text":
                    if "Task 1" in c.get("text", ""):
                        print(f"First answer written at turn {turn}")
                        sys.exit(0)
print(f"Never found — checked {turn} turns")
EOF
```

---

## Scoring task accuracy (blind)

Score each result file independently before comparing agent labels. Use this rubric per task (same as previous benchmark):

| Dimension | Max | Criteria |
|-----------|-----|----------|
| Accuracy | 10 | Is the answer factually correct? Does it identify the right code? |
| Depth | 10 | Does it explain *how* the code works, not just *where* it is? |
| Code precision | 10 | Are file paths and line references exact and correct? |
| Notable findings | 10 | Does it surface non-obvious design decisions or bugs? |
| Format | 10 | Is output clean, consistent, and complete across all 20 tasks? |

Total per agent: /50 across all tasks, then average per task for /10.

Record results in BENCHMARK_SCORES.md.

---

## Expected results table

Fill this in after running:

| Metric | Agent A (grep) | Agent B (semantic) | Agent C (map) |
|--------|---------------|-------------------|---------------|
| Waste ratio | ~2.7x (known) | ~2.5x (known) | ? |
| Orientation turns | ? | ? | ? |
| Total tokens | ~713k (known) | ~484k (known) | ? |
| Total turns | ~25 (known) | ~19 (known) | ? |
| Accuracy score | ~43/50 (known) | ~46/50 (known) | ? |

---

## Tasks

The 20 tasks below are identical to the original benchmark. Do not modify them — consistency is required for a valid comparison.

---

## Task 1: Embedding Provider Dispatch

How does the tool decide at runtime which embedding backend to use (local ONNX model vs. OpenAI API)? Trace the decision path from CLI input to the actual embedding call.

---

## Task 2: Database Schema Initialization

What tables and indexes does the tool create in SQLite on first run? How are vector embeddings stored alongside code chunks — are they in the same table or separate?

---

## Task 3: Tree-sitter Chunking Strategy

When parsing a Rust source file, how does the chunker decide where one chunk starts and another ends? What AST node types trigger a chunk boundary, and what is the fallback when tree-sitter fails to parse a file?

---

## Task 4: Cosine Similarity and Result Ranking

Where is the similarity score computed — inside SQLite (via sqlite-vec), in Rust, or delegated to the embedding provider? How are results sorted and how is the top-N limit enforced?

---

## Task 5: Incremental Indexing / Change Detection

When a file is modified after the initial index is built, how does the tool detect the change and update only the affected chunks? What identifier is used to track file state (hash, mtime, git object ID)?

---

## Task 6: CLI Command Structure and Subcommand Routing

List every subcommand exposed by the CLI. For each one, identify its Clap struct, its handler function, and any notable arguments or flags it accepts.

---

## Task 7: ONNX Model Loading and Inference

Walk through the ONNX inference path: how is the model file located, loaded, and invoked to produce an embedding vector? How is the tokenizer handled?

---

## Task 8: OpenAI API Request Construction

How is the HTTP request to the OpenAI embeddings endpoint built? What model name is used, how is the API key sourced, and how is the response deserialized into a float vector?

---

## Task 9: Semantic Branch Diffing

The `semantic_branch` module compares branches semantically. Explain the full algorithm: what inputs it takes, how it retrieves embeddings for changed chunks, and how it produces its output score or diff.

---

## Task 10: Language Detection and Parser Selection

Given an arbitrary file path, how does the tool choose which tree-sitter grammar to use? Is detection based on file extension, shebang, content sniffing, or something else? What happens with unsupported languages?

---

## Task 11: Chunk Deduplication

If the same logical code block appears in two files (copy-paste), or if a file is indexed twice, does the system deduplicate chunks? What key is used for deduplication and where is uniqueness enforced?

---

## Task 12: Progress Reporting During Indexing

How is the progress bar or progress indicator implemented during the indexing phase? What crate or mechanism is used, and at what granularity is progress reported (per file, per chunk, per batch)?

---

## Task 13: Error Propagation Strategy

Trace how an error originating deep in the embedding pipeline (e.g., a network failure during an OpenAI call) propagates back to the CLI entry point. Does the tool use `anyhow`, `thiserror`, `?`, or manual wrapping?

---

## Task 14: Vector Storage Format in SQLite

How are raw float vectors serialized before being written to SQLite? What binary format or encoding is used (raw bytes, base64, bincode, sqlite-vec native format)? And how are they deserialized on read?

---

## Task 15: `enable claude` Command Behavior

What exactly does the `enable claude` command do step by step? What files does it create or modify, what content does it inject, and under what conditions does it skip or overwrite existing content?

---

## Task 16: Embedding Dimensionality and Model Config

Where is the embedding vector dimension defined or discovered? Is it hardcoded, read from a config file, or inferred from the model at runtime? How would adding a new model with a different dimension affect the schema?

---

## Task 17: Git Integration — Reading the Working Tree

How does the tool traverse the repository to find files to index? Does it use `git ls-files`, walk the filesystem directly, or use the `gix` crate to read git objects? How are `.gitignore` rules respected?

---

## Task 18: Batch Embedding Requests

When indexing a large repository, are embedding requests sent one chunk at a time or batched? If batched, what is the maximum batch size and where is that limit enforced?

---

## Task 19: Config File Resolution

Where does the tool store its configuration (model choice, API key path, DB location)? How does it resolve the config file — fixed path, XDG base dirs, env vars, or per-repo? What happens when no config exists?

---

## Task 20: Output Formatting for `grep` Results

How are `git-semantic grep` results formatted for terminal output? What information is shown per result (score, file, line range, snippet), how is truncation handled for long chunks, and is there any color/highlighting applied?
