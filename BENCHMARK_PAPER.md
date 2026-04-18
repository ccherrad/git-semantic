# Spatial Map Benchmark — Multi-Codebase Agent Navigation Study

**Status**: Data collection in progress
**Target venue**: arXiv cs.SE, then MSR 2027

---

## Abstract

AI coding agents accumulate context across turns, causing per-turn token cost to grow monotonically over a session. We introduce the *waste ratio* — the ratio of average tokens per turn in the last three turns to the first five — as a formal measure of session efficiency degradation. We hypothesize that a pre-computed spatial map of a codebase, committed to a git branch and queried at session start, reduces waste ratio and orientation cost without sacrificing task accuracy. We test this hypothesis across three codebases (Rust, Python, TypeScript), two agent configurations (grep, map+get), and three independent runs per combination (18 sessions total, 60 investigation tasks). Tasks are sampled from real GitHub issues. Results are analyzed using Wilcoxon signed-rank tests with effect sizes reported.

---

## 1. Hypotheses

**H1 — Waste ratio**: A map-equipped agent completes investigation tasks with a lower per-turn token growth rate (waste ratio) than a search-only agent, holding task set and codebase constant.

**H2 — Orientation cost**: A map-equipped agent requires fewer turns before producing its first substantive answer than a grep agent.

**H3 — Task accuracy**: A map-equipped agent produces answers of equal or higher quality than a grep agent despite lower token consumption.

---

## 2. Experimental Design

### 2.1 Codebases

| ID | Repository | Language | LOC (approx) | Structure |
|----|-----------|----------|--------------|-----------|
| C1 | [git-semantic](https://github.com/ccherrad/git-semantic) | Rust | ~3,000 | Small CLI, 7 modules |
| C2 | [Textual](https://github.com/Textualize/textual) | Python | ~35,000 | TUI framework, 8 subsystems |
| C3 | [Hono](https://github.com/honojs/hono) | TypeScript | ~17,000 | Web framework, multi-runtime adapters |

C1 has existing single-run baseline numbers (Agent A: 3.7x, Agent C: 3.3x). These are used as a prior; 3 fresh runs are still required to obtain mean ± SD.

### 2.2 Agent configurations

| ID | Label | Tools allowed | Enforcement |
|----|-------|--------------|-------------|
| A | grep | all tools, no restrictions | None |
| C | map | `Bash`, `Write` only — forces git-semantic | `--allowedTools "Bash,Write"` |

### 2.3 Runs

Each agent × codebase combination: 3 independent runs with fresh sessions.
Total: 3 codebases × 2 agents × 3 runs = **18 sessions**.

### 2.4 Model

Claude Sonnet 4.6 for all runs. Temperature: default. No system prompt beyond the task file and CLAUDE.md.

### 2.5 Task sets

20 investigation tasks per codebase (60 total). Each task:
- Requires locating and explaining a specific mechanism in the code
- Is answerable by reading 2–3 files
- Has a verifiable ground truth (a specific file and function that answers it)
- Is derived from a real GitHub issue where possible

Full task sets in Section 6 (Appendix C).

---

## 3. Metrics

### 3.1 Primary metrics

| Metric | Definition | Source |
|--------|-----------|--------|
| Waste ratio | avg tokens/turn (last 3) ÷ avg tokens/turn (first 5) | `git-semantic usage` on session JSONL |
| Orientation turns | Turns elapsed before first Task 1 answer is written | Orientation parser (Appendix A) |
| Total tokens | Sum of all assistant turn token counts | `git-semantic usage` |

### 3.2 Secondary metrics

| Metric | Definition | Source |
|--------|-----------|--------|
| Task accuracy | Blind score /50 across 5 dimensions | 2 independent raters |
| Inter-rater reliability | Cohen's kappa on per-task scores | Computed post-scoring |
| Map edge precision | Fraction of edges verified as real cross-file calls | Manual protocol (Appendix B) |

### 3.3 Accuracy rubric (per task, per rater)

| Dimension | Max | Criteria |
|-----------|-----|----------|
| Accuracy | 10 | Identifies the correct file, function, and mechanism |
| Depth | 10 | Explains *how* the code works, not just *where* it is |
| Code precision | 10 | File paths and line references are exact |
| Notable findings | 10 | Surfaces non-obvious design decisions or constraints |
| Format | 10 | Clean, complete, consistent across all 20 tasks |

**Total per run: /50.** Report mean ± SD across 3 runs per agent per codebase.

---

## 4. Statistical Analysis Plan

- **Primary test**: Wilcoxon signed-rank test on waste ratio (A vs C) — non-parametric, appropriate for small N
- **Effect size**: Cohen's d for waste ratio and accuracy
- **Significance threshold**: p < 0.05
- **Inter-rater reliability gate**: if κ < 0.6, revise rubric and re-score before reporting accuracy results
- **Report format**: mean ± SD, 95% CI, W statistic, p-value, Cohen's d for all comparisons

---

## 5. How to Run

### 5.1 Prerequisites

```bash
# Install git-semantic
cargo install gitsem

# Clone the three codebases into separate directories
git clone https://github.com/ccherrad/git-semantic bench/c1
git clone https://github.com/Textualize/textual bench/c2
git clone https://github.com/honojs/hono bench/c3

# Index C2 and C3 (C1 is already indexed if you cloned the semantic branch)
cd bench/c2 && git-semantic index
cd bench/c3 && git-semantic index
```

### 5.2 Agent A — grep

No hook setup. Run once per codebase per run number (1, 2, 3).

```bash
# Example: C2, run 1
cd bench/c2
claude -p "$(cat <<'EOF'
You are investigating this codebase. Use grep or rg for all code searches.
Do not use any git-semantic commands.

Read BENCHMARK_C2_TASKS.md. For each of the 20 tasks:
1. Run the search commands needed to answer it.
2. Append your findings to BENCHMARK_RESULTS_A_R1.md
   Each section must start with: ## Task N: <title>
   Include: commands run, file:line references, explanation, notable findings.

Work sequentially. Do not skip any task.
EOF
)"
```

Replace `C2` → `C1` or `C3`, `R1` → `R2` or `R3` for other runs.

### 5.3 Agent C — map + get

```bash
cd bench/c2

# Install full map+get rules into CLAUDE.md
git-semantic enable claude

# Hydrate local DB
git-semantic hydrate

claude -p "$(cat <<'EOF'
You are investigating this codebase. Follow CLAUDE.md exactly.

Read BENCHMARK_C2_TASKS.md. For each of the 20 tasks:
1. Orient: git-semantic map "<query>"
2. Retrieve: git-semantic get <file:start-end> (max 3 calls per task)
3. Search: git-semantic grep "<query>" only if map was insufficient
4. Append your findings to BENCHMARK_RESULTS_C_R1.md
   Each section must start with: ## Task N: <title>
   Include: commands run, file:line references, explanation, notable findings.

Work sequentially. Do not skip any task.
EOF
)"
```

### 5.5 Collecting metrics after each run

```bash
# Waste ratio and token counts
git-semantic usage -s 1

# Orientation cost — find session JSONL path from usage output, then:
python3 scripts/orientation.py ~/.claude/projects/<project-dir>/<SESSION_ID>.jsonl
```

---

## 6. Results (fill after runs)

### 6.1 Waste ratio — mean ± SD across 3 runs

| Codebase | Agent A (grep) | Agent C (map) |
|----------|---------------|---------------|
| C1 git-semantic | 3.7x (R1), — , — | 3.3x (R1), — , — |
| C2 Textual | — | — |
| C3 Hono | — | — |
| **Mean ± SD** | — | — |

### 6.2 Orientation turns — mean ± SD

| Codebase | Agent A | Agent C |
|----------|---------|---------|
| C1 | — | — |
| C2 | — | — |
| C3 | — | — |
| **Mean ± SD** | — | — |

### 6.3 Total tokens — mean across 3 runs (k tokens)

| Codebase | Agent A | Agent C |
|----------|---------|---------|
| C1 | 1,100k (R1) | 1,800k (R1) |
| C2 | — | — |
| C3 | — | — |

### 6.4 Task accuracy — mean ± SD across 3 runs, 2 raters (/50)

| Codebase | Agent A | Agent C | κ |
|----------|---------|---------|---|
| C1 | — | — | — |
| C2 | — | — | — |
| C3 | — | — | — |

### 6.5 Statistical tests

| Comparison | Metric | W | p | Cohen's d | Significant |
|------------|--------|---|---|-----------|-------------|
| A vs C | Waste ratio | — | — | — | — |
| A vs C | Orientation turns | — | — | — | — |
| A vs C | Accuracy | — | — | — | — |

---

## 7. Threats to Validity

### Internal validity

- **Order effects**: Task order is fixed and identical across all agents and runs. Any order effect is constant and does not confound between-agent comparisons.
- **Prompt sensitivity**: Run prompts are fixed before data collection begins and are identical across all 3 runs per agent configuration.
- **Model non-determinism**: LLM outputs vary across runs at fixed temperature. Mitigated by 3 independent runs; variance is reported.

### External validity

- **Single model**: All results are for Claude Sonnet 4.6. Generalization to other models is not claimed. Replication with GPT-4o is planned as future work.
- **Codebase selection**: 3 codebases selected for architectural clarity and language diversity. Results may not generalize to codebases with poor directory structure or high cross-language mixing.
- **Map quality dependence**: Agent C results are conditioned on the quality of the spatial map. Map edge precision is reported separately (Appendix B) to make this dependence explicit.

### Construct validity

- **Waste ratio as efficiency proxy**: A waste ratio of 1.0x is achievable by an agent that does nothing useful. Accuracy score is the necessary counterbalance — efficiency and accuracy are always reported together.
- **Task representativeness**: Investigation tasks may not represent all real developer workflows (debugging, feature addition, refactoring). Tasks sampled from real GitHub issues reduce but do not eliminate this threat.

---

## Appendix A — Orientation Cost Parser

Detects the first turn at which the agent writes a Task 1 answer to the results file.

```python
#!/usr/bin/env python3
import json, sys

path = sys.argv[1]
turn = 0

with open(path) as f:
    for line in f:
        try:
            r = json.loads(line)
        except Exception:
            continue
        if r.get("type") != "assistant":
            continue
        turn += 1
        content = r.get("message", {}).get("content", [])
        for c in content:
            if not isinstance(c, dict):
                continue
            if c.get("type") == "tool_use":
                inp = str(c.get("input", {}))
                if "BENCHMARK_RESULTS" in inp and ("Task 1" in inp or "## Task" in inp):
                    print(f"orientation_turns={turn}")
                    sys.exit(0)
            if c.get("type") == "text":
                if "## Task 1" in c.get("text", ""):
                    print(f"orientation_turns={turn}")
                    sys.exit(0)

print(f"orientation_turns=NOT_DETECTED  # checked {turn} turns — verify manually")
```

**Usage:**
```bash
python3 scripts/orientation.py \
  ~/.claude/projects/-Users-you-bench-c2/SESSION_ID.jsonl
```

---

## Appendix B — Map Edge Precision Protocol

Verifies that edges extracted by `git-semantic map` correspond to real cross-file symbol references.

For each codebase after indexing, run `git-semantic map` and randomly sample 10 edges from the output. For each edge of the form `A → B via symbol`:

1. Open file A — confirm `symbol` appears as a call site or import
2. Open file B — confirm `symbol` is defined there (function, struct, trait, enum, or const)
3. Mark **correct** if both conditions hold, **false positive** otherwise

```
precision = correct_count / 10
```

| Codebase | Sampled edges | Correct | Precision |
|----------|--------------|---------|-----------|
| C1 git-semantic | 10 | — | — |
| C2 Textual | 10 | — | — |
| C3 Hono | 10 | — | — |

**Interpretation**: precision < 0.7 on a codebase means edge quality is insufficient for that language and results from Agent C on that codebase should be treated with caution.

---

## Appendix C — Task Sets

---

### C1 — git-semantic (Rust)

**Task 1: Embedding Provider Dispatch**
How does the tool decide at runtime which embedding backend to use (local ONNX model vs. OpenAI API)? Trace the decision path from CLI input to the actual embedding call.

**Task 2: Database Schema Initialization**
What tables and indexes does the tool create in SQLite on first run? How are vector embeddings stored alongside code chunks — are they in the same table or separate?

**Task 3: Tree-sitter Chunking Strategy**
When parsing a Rust source file, how does the chunker decide where one chunk starts and another ends? What AST node types trigger a chunk boundary, and what is the fallback when tree-sitter fails to parse a file?

**Task 4: Cosine Similarity and Result Ranking**
Where is the similarity score computed — inside SQLite (via sqlite-vec), in Rust, or delegated to the embedding provider? How are results sorted and how is the top-N limit enforced?

**Task 5: Incremental Indexing / Change Detection**
When a file is modified after the initial index is built, how does the tool detect the change and update only the affected chunks? What identifier is used to track file state?

**Task 6: CLI Command Structure and Subcommand Routing**
List every subcommand exposed by the CLI. For each one, identify its Clap struct, its handler function, and any notable arguments or flags it accepts.

**Task 7: ONNX Model Loading and Inference**
Walk through the ONNX inference path: how is the model file located, loaded, and invoked to produce an embedding vector? How is the tokenizer handled?

**Task 8: OpenAI API Request Construction**
How is the HTTP request to the OpenAI embeddings endpoint built? What model name is used, how is the API key sourced, and how is the response deserialized into a float vector?

**Task 9: Semantic Branch Diffing**
The `semantic_branch` module manages the orphan branch. Explain the full algorithm: how it reads and writes chunk files, how it detects changes, and how it structures the commit.

**Task 10: Language Detection and Parser Selection**
Given an arbitrary file path, how does the tool choose which tree-sitter grammar to use? Is detection based on file extension, shebang, content sniffing, or something else? What happens with unsupported languages?

**Task 11: Chunk Deduplication**
If a file is indexed twice, does the system deduplicate chunks? What key is used for deduplication and where is uniqueness enforced?

**Task 12: Progress Reporting During Indexing**
How is the progress bar implemented during the indexing phase? What crate is used, and at what granularity is progress reported?

**Task 13: Error Propagation Strategy**
Trace how an error originating deep in the embedding pipeline (e.g. a network failure during an OpenAI call) propagates back to the CLI entry point. Does the tool use `anyhow`, `thiserror`, `?`, or manual wrapping?

**Task 14: Vector Storage Format in SQLite**
How are raw float vectors serialized before being written to SQLite? What binary format is used (raw bytes, base64, bincode, sqlite-vec native)? How are they deserialized on read?

**Task 15: `enable claude` Command Behavior**
What exactly does `enable claude` do step by step? What files does it create or modify, what content does it inject, and under what conditions does it skip or overwrite existing content?

**Task 16: Embedding Dimensionality and Model Config**
Where is the embedding vector dimension defined or discovered? Is it hardcoded, read from config, or inferred from the model at runtime? How would adding a new model with a different dimension affect the schema?

**Task 17: Git Integration — Reading the Working Tree**
How does the tool traverse the repository to find files to index? Does it use `git ls-files`, walk the filesystem, or use the `gix` crate? How are `.gitignore` rules respected?

**Task 18: Batch Embedding Requests**
When indexing a large repository, are embedding requests sent one chunk at a time or batched? If batched, what is the maximum batch size and where is that limit enforced?

**Task 19: Config File Resolution**
Where does the tool store its configuration? How does it resolve the config — fixed path, XDG base dirs, env vars, or per-repo git config? What happens when no config exists?

**Task 20: Output Formatting for `grep` Results**
How are `git-semantic grep` results formatted for terminal output? What information is shown per result, how is truncation handled for long chunks, and is there any color or highlighting applied?

---

### C2 — Textual (Python)

**Task 1: Event Routing Through Message Pump**
Trace how key press events flow from the driver through the message pump to widgets. Where is the message pump module that dispatches events, how does `on_key()` get routed to the active widget in focus, and what determines whether an event bubbles up to the parent screen?

**Task 2: CSS Style Resolution and Specificity**
Trace the CSS style cascade in Textual. Where is CSS specificity calculated, how does the style resolver determine which rule wins when multiple selectors match a widget, and what data structure holds the computed styles for a single widget?

**Task 3: Widget Tree Navigation with DOMQuery**
Trace DOM-style widget queries. Where is `DOMQuery` implemented, how does the `exclude()` method filter out widgets, and why might an empty result set be incorrectly treated as matching all elements?

**Task 4: Key Binding Resolution**
Trace how key bindings are resolved when a user presses a key. Where is the module that stores action bindings, how does the input handler match a pressed key sequence to a binding, and what happens when multiple bindings could match the same key?

**Task 5: Terminal Driver Abstraction**
Trace the driver abstraction that allows Textual to support multiple terminal backends. Where is the base Driver class, what methods must each driver implement, and how does the app choose which driver to instantiate at runtime?

**Task 6: Reactive Attribute Change Propagation**
Trace how reactive attributes trigger re-renders. Where is the reactive property system defined, how does setting a reactive attribute trigger a watch callback, and what prevents infinite loops when a watcher modifies another reactive attribute?

**Task 7: Text Selection Offset Calculation**
Trace text selection positioning. Where is the text measurement logic that calculates character offsets, how does the renderer account for tab width in offset calculations, and what structure stores selection start and end positions?

**Task 8: Widget Render Path and Compositor**
Trace how a widget's render output gets painted to the screen. Where is the compositor or paint routine, how does it composite multiple widget render regions, and what data structure represents a single painted line?

**Task 9: Application Lifecycle and Screen Stack**
Trace app startup through to the first screen render. Where is the app class that manages the lifecycle, what sequence of methods runs during `__init__`, `run()`, and `on_mount()`, and how does the screen stack determine which screen is active?

**Task 10: Layout System Size Negotiation**
Trace how the layout system calculates widget dimensions. Where is the layouts subsystem that manages box model sizing, what is the algorithm for width and height negotiation between parent and children, and how are constraints applied?

**Task 11: Animation Frame Scheduling**
Trace how animations are scheduled and executed. Where is the animation scheduler that batches frame updates, how does it know when to trigger the next frame, and what prevents animations from blocking the event loop?

**Task 12: Markdown Rendering Pipeline**
Trace how Markdown is parsed and rendered. Where is the Markdown parser, what renderable objects does it produce, and how are those renderables painted to the screen?

**Task 13: Input Widget Focus Management**
Trace focus management for input widgets. Where is the focus tracking code, how does `set_focus()` route input to the focused widget, and what validates that the target widget can receive focus?

**Task 14: Color Parsing and Theme Application**
Trace color definitions from theme through to terminal output. Where is the color parsing module, how are theme colors resolved to ANSI or RGB terminal codes, and what data structure maps theme names to color values?

**Task 15: Window Title Setting**
Trace the window title command path. Where is `set_window_title()` implemented in each driver, what escape sequence does it send, and why might the terminal ignore it in certain environments?

**Task 16: Mouse Event Coordinate Mapping**
Trace mouse input from raw coordinates to widget-relative events. Where is the mouse event handler in the driver, how are screen-space coordinates mapped to widget-relative coordinates, and what determines which widget receives a mouse event?

**Task 17: Unicode Decoding in Input Thread**
Trace UTF-8 byte handling in the input thread. Where is the input thread that reads terminal bytes, how does it handle incomplete UTF-8 sequences, and what exception handling prevents a crash on malformed input?

**Task 18: DataTable Row and Viewport Rendering**
Trace how DataTable renders cells and manages scrolling. Where is the DataTable widget implementation, how does it lay out rows and columns, and what data structure tracks which cells are visible in the current viewport?

**Task 19: TabbedContent State Management**
Trace tab switching in TabbedContent. Where is TabbedContent implemented, how does it track the active tab index, and what message is sent when a tab button is pressed?

**Task 20: Text Wrapping and Line Breaking**
Trace text wrapping logic. Where is `compute_wrap_offsets()` or equivalent implemented, what algorithm determines line break points, and how are word boundaries handled?

---

### C3 — Hono (TypeScript)

**Task 1: RegExpRouter Pattern Matching**
Trace how RegExpRouter matches incoming request paths to route handlers. Where is the RegExpRouter implementation, how does it compile route patterns into regular expressions, and what is the priority order when multiple patterns match the same path?

**Task 2: Middleware Composition and Execution Order**
Trace middleware execution through a request. Where is the middleware chain composition code, how does each middleware call `next()` to pass control forward, and what data structure represents the middleware stack?

**Task 3: Context Object Lifecycle**
Trace the Context object from request entry to response return. Where is the Context class defined, what properties are populated on creation, and at what point in the request cycle is it available to handlers?

**Task 4: TrieRouter vs RegExpRouter Catch-All Handling**
Trace the difference between TrieRouter and RegExpRouter behavior on catch-all routes. Where is each router implemented, how do they differ in handling `/**` patterns, and when should each be used?

**Task 5: Runtime Adapter Abstraction**
Trace how adapters abstract different JavaScript runtimes. Where is the base adapter interface, what methods must Node.js, Cloudflare Workers, and Bun adapters implement, and how does the app select which adapter to use?

**Task 6: RPC Type Inference**
Trace type inference in RPC endpoints. Where is the RPC type inference logic, how does it extract return types from handler functions, and why might arrow functions with Promises break the inference?

**Task 7: Cookie Parsing and Management**
Trace cookie handling. Where is the cookie parser, how does it parse the `Cookie` header and store cookies in the Context, and what is the API for setting response cookies?

**Task 8: Cookie Encryption Integration Point**
Trace where cookie encryption would integrate with the existing cookie middleware. Where does the cookie set/get flow, what would be the right place to intercept for encryption, and how would the encrypted value be serialized?

**Task 9: Route Existence Check Before Middleware**
Trace where route existence checking could be inserted into the request pipeline. Where does routing resolution happen relative to middleware execution, and what internal API would allow middleware to inspect whether a route handler exists?

**Task 10: 405 Method Not Allowed Generation**
Trace HTTP method validation. Where is the routing logic that checks method support, how does it determine if a path exists with a different method, and where should the 405 response be generated?

**Task 11: Static File Serving and Path Traversal Protection**
Trace the static file middleware. Where is it implemented, how does it resolve file paths from the request URL, and what checks prevent path traversal attacks?

**Task 12: JWT Middleware Token Validation**
Trace JWT authentication middleware. Where is the JWT verification code, how does it extract the token from the request, and what happens when token validation fails?

**Task 13: Not Found Handler and Default Error Response**
Trace 404 and error handling. Where is the default not-found handler invoked, how can users register a custom 404 handler, and what is the default response format?

**Task 14: Streaming Response Body Handling**
Trace streaming response support. Where is the streaming response code, how does it write chunks to the response body, and what prevents backpressure issues?

**Task 15: Vercel Adapter Type Compatibility**
Trace the Vercel adapter handler signature. Where is the Vercel adapter defined, what is the handler function's type signature, and how does it map to Next.js route handler types?

**Task 16: Helper Function Response Formatting**
Trace the helper utilities in `src/helper/`. Where are `html()`, `text()`, and `json()` implemented, how do they format responses, and what Content-Type headers do they set?

**Task 17: Request Parameter Parsing**
Trace parameter extraction from query strings, JSON bodies, and URL path segments. Where is each parsed, and how does the Context expose them to handlers?

**Task 18: Route Path Retrieval at Middleware Time**
Trace how `routePath()` works. Where is it implemented, what does its numeric parameter represent, and at what point in the request lifecycle is the matched route path available?

**Task 19: Colon Escaping in Custom Method Routes**
Trace route parsing for custom HTTP method syntax. Where is the route pattern parser, how does it currently handle colons in path segments, and what change would support escaped colons for Google-style custom methods?

**Task 20: WebSocket Connection and Cleanup**
Trace WebSocket support. Where is the WebSocket helper that handles upgrades and message routing, how does it detect disconnections, and what cleanup is performed when a connection closes?
