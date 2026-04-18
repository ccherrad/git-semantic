# State of the Art — Codebase Navigation and Agent Context Efficiency

A survey of prior work relevant to pre-computed spatial maps for AI coding agent orientation. Covers codebase navigation tools, structural code representations, context efficiency research, and agent memory systems.

---

## 1. Codebase Navigation for AI Agents

### Production tools

**Aider — Repository Map**
Aider pre-computes a structural summary of the codebase before each session: function signatures, class names, file structure. This map is given to the agent as upfront context. The token budget is split: largest portion to the repo map, remainder to selected file contents.
- Regenerated per session — not persisted, not shared across users
- No call edges, no subsystem clustering
- No measurement of session efficiency over time
- *Closest prior work to our approach*

**Cursor — Merkle Tree Index**
Cursor builds a codebase index using a Merkle tree to detect file changes via git diff. Files are chunked by AST, embedded as vectors, and stored in a proprietary vector store (Turbopuffer). Queries trigger nearest-neighbor retrieval.
- Index is local and ephemeral — not committed to version control
- No explicit spatial structure — flat vector retrieval
- No session efficiency metric

**GitHub Copilot Workspace**
Analyzes what information is needed for a task, then selects search strategies (semantic search, grep, file search, usages) in parallel. Index is backed by GitHub's remote infrastructure.
- Sophisticated multi-tool retrieval strategy
- No pre-computed spatial map — discovers structure per task
- Index is remote and non-portable

**Sourcegraph Cody**
Uses Sourcegraph's centralized code search engine. Pre-processes queries into tokens, scans repos, ranks snippets by relevance.
- Depends on a centralized service
- No optimization for agent memory persistence or session efficiency

**Continue.dev**
Uses the Language Server Protocol for context: function definitions, imports, type information. File history awareness. Autocomplete uses debouncing and caching.
- LSP integration is language-specific
- Not a general spatial map
- No cross-session sharing

### Benchmarks and academic tools

**SWE-bench** (Jimenez et al., 2024)
A benchmark of real GitHub issues requiring agents to navigate repositories and produce patches. Top-performing systems use BM25 keyword search combined with semantic search. Agents rely on `grep`, `find`, and file reads to locate relevant code.
- No pre-computed map — agents discover structure at runtime
- Measures task completion, not session token efficiency

**SWE-agent: Agent-Computer Interfaces** (Yang et al., NeurIPS 2024)
Introduces a custom agent-computer interface with tools like `find_file`, `search_file`, `search_dir`. Agents build understanding incrementally via tool calls.
- Focuses on tool interface design, not pre-computed structural representation
- No measurement of context accumulation

**CrossCodeEval** (Ding et al., NeurIPS 2023)
A 10,000-example multilingual benchmark for cross-file code completion. Finding: even top retrieval methods fall short when relevant context spans multiple files.
- Establishes that cross-file context retrieval is a hard unsolved problem
- Does not address agent session efficiency

---

## 2. Structural Code Representations

### Knowledge graph approaches

**Prometheus: Long-Horizon Codebase Navigation** (arXiv 2507.19942, 2025)
Represents a repository as a unified knowledge graph. Uses a context engine and working memory to retain and reuse explored contexts across turns. Achieves 74.4% on SWE-bench Verified.
- Addresses multi-turn context accumulation via in-session working memory
- Knowledge graph built dynamically during agent execution (requires LLM inference)
- Not committed to version control — ephemeral per session
- No waste ratio measurement

**GraphCodeAgent** (arXiv 2504.10046, 2025)
Dual-graph approach: a Requirement Graph and a Structural-Semantic Code Graph. Agents perform multi-hop reasoning over explicit graphs rather than vector retrieval.
- Explicit structural graphs for navigation
- Graphs generated via LLM inference — expensive and non-deterministic
- Not a static artifact; rebuilt per session

**LocAgent** (ACL 2025)
Graph-guided LLM agents for fault localization. Builds a structural graph of the codebase and uses it to narrow the search space for bug localization.
- Structural graph for localization, not general navigation
- Dynamic construction, not persisted

**Codebase-Memory MCP** (DeusData, 2025)
Parses codebases with tree-sitter into a persistent SQLite knowledge graph. Exposes 14 structural query tools via the Model Context Protocol. Supports 66 languages, sub-millisecond queries, claims 99% fewer tokens versus reading raw files.
- Pre-computed and persistent ✓
- Exposed via MCP server — requires a running service
- Not committed to version control — not shareable via git
- No session efficiency benchmark

**Graphify** (2026)
Builds queryable knowledge graphs from code, documentation, and other sources using tree-sitter AST extraction combined with LLM-based concept extraction. Claims 71.5× token reduction versus reading raw files. Uses Leiden clustering (graph-topology-based, no embeddings).
- Pre-computed graph ✓
- Privacy-focused — no code sent to external services
- Not committed to version control
- Relies on LLM inference for concept extraction

**Theory of Code Space (ToCS)** (arXiv 2603.00601, 2026)
Evaluates whether agents construct and maintain coherent architectural beliefs during codebase exploration. Finding: strong models (Claude Sonnet 4.6, GPT-5) can build mental models if they externalize them; weaker models fail at externalization.
- Motivates pre-computed externalized structure — agents benefit from having architecture described upfront
- Does not propose a mechanism for providing that structure

**Codified Context Infrastructure** (arXiv 2602.20478, 2026)
For a 108,000-line C# system: hot-memory constitution (conventions), 19 domain-expert agents, 34 cold-memory specification documents. Knowledge-to-code ratio: 24.2%.
- Multi-tiered memory architecture explicitly written for machine consumption
- Manually authored — not automatically extracted from code
- Not a spatial map

### Chunking strategies

**cAST: Enhancing Code Retrieval via AST-Based Chunking** (arXiv 2506.15655)
Splits code at semantic boundaries (functions, classes, control structures) using AST parsing. Chunks are syntactically valid; metadata headers include file path and hierarchy.
- Standard for semantic chunking — used by Cursor, Aider, Continue.dev, and codebase-memory-mcp
- Addresses chunk quality, not structural navigation or session efficiency

**RepoCoder, RepoFusion, CrossCodeEval** (various, 2022–2023)
Repo-level code completion work using retrieval of cross-file context. RepoCoder uses iterative retrieval; RepoFusion fuses repository-level context into code generation.
- Focus on completion quality, not agent session efficiency
- No structural map — flat retrieval from repository

---

## 3. Context Window Efficiency

### Empirical measurements

**"70% of Tokens Are Waste"** (Lessi, DEV Community, 2026)
Tracked token consumption across 42 Claude Sonnet 4.6 sessions on a FastAPI codebase (~800 Python files). Finding: agents lack a codebase map and read the same files repeatedly on every prompt, unlike humans who read once. Context bloat worsens after turn 15.
- Empirical observation of the waste problem
- No formal metric, no proposed solution, no benchmark
- *Direct empirical motivation for the waste ratio metric*

### Compression approaches

**ACON: Optimizing Context Compression for Long-Horizon LLM Agents** (arXiv 2510.00615, NeurIPS 2024/2025)
Unified framework for compressing both environment observations and interaction histories. Uses compression guideline optimization: paired successful/failed trajectories inform compression rules.
- Addresses context accumulation post-hoc via compression
- Our approach is preventive — avoid accumulation via pre-computed orientation

**Factory.ai Context Compression Studies** (Factory.ai, 2025)
Compares anchored iterative summarization versus full summarization. Finding: preserve 10% verbatim as active working memory, summarize older context. Achieves 60–80% context size reduction.
- Summarization-based, not structure-based
- Applied after accumulation, not before

**Lost in the Middle** (Liu et al., Stanford, 2023 — arXiv 2307.03172)
LLMs show U-shaped accuracy across context position: high at edges, low in the middle. Performance drops ~30% when relevant information is in the middle of a long context.
- Motivates strategic positioning of retrieved chunks
- A pre-computed map enables the agent to position relevant chunks at context edges rather than discovering them mid-session

---

## 4. Agent Memory and Session Management

**MemGPT: Towards LLMs as Operating Systems** (Packer et al., 2023 — arXiv 2310.08560)
Virtual context management with hierarchical memory: main context plus archival storage. Agent self-directs memory editing via tool calls — decides what to store, summarize, or discard.
- Persistent external memory for long-running agents
- Conversation memory, not structural code memory
- Complementary to a spatial map rather than competing

**A-Mem: Agentic Memory System** (arXiv 2502.12110, 2025)
Long-term memory for agents that auto-updates on interactions using dynamic memory networks inspired by Zettelkasten.
- Conversational memory — tracks what the agent has done
- Not structural memory (code topology)

**Mem0** (arXiv 2504.19413, 2025)
Intelligent memory layer for AI agents and assistants. Auto-extracts and updates memory from interactions.
- Same distinction: conversational vs structural memory

---

## 5. Novelty Assessment

### Component-level comparison

| Component | Prior work | Our approach | Novel |
|-----------|-----------|--------------|-------|
| Pre-computed codebase index | Cursor, Aider, Codebase-Memory MCP | ✓ | No |
| Committed to git as versioned artifact | None found | ✓ | **Yes** |
| Directory-level clustering | Graphify (Leiden), modern IDEs (implicit) | ✓ | No |
| Function name labels | All RAG systems | ✓ | No |
| Cross-file call edges (static extraction) | Prometheus, GraphCodeAgent (LLM-generated) | ✓ static | **Partial** |
| Waste ratio as a formal session metric | Informal observation only | ✓ formalized | **Yes** |
| Multi-session reuse via shared artifact | None found | ✓ | **Yes** |
| Benchmark of map vs no-map efficiency | None found | ✓ | **Yes** |

### White space

Four things that do not exist in prior work:

1. **A codebase map committed to version control** — every prior tool regenerates or stores indices locally. Committing to git enables version-controlled snapshots, team sharing, offline-first workflows, and reproducible agent reasoning over a fixed map.

2. **Waste ratio as a formal metric** — the token growth rate problem has been observed empirically (70% waste, context rot after turn 15) but never formalized as a benchmarkable metric or targeted by an experimental intervention.

3. **Static extraction versus LLM-generated graphs** — Prometheus and GraphCodeAgent build knowledge graphs via LLM inference (expensive, non-deterministic, requires API access). Our approach uses tree-sitter name matching (free, deterministic, reproducible). No paper has compared these two approaches on agent session efficiency.

4. **Cross-codebase, multi-run benchmark of session efficiency** — no existing benchmark measures waste ratio across multiple codebases, multiple agent configurations, and multiple runs with statistical testing.

---

## 6. Positioning Statement

> While recent work (Prometheus, GraphCodeAgent) builds dynamic knowledge graphs during agent execution, and production tools (Cursor, Aider) regenerate indices per session, this work proposes a lightweight, statically-extracted, version-controlled spatial index — directory clustering, function-name labels, and cross-file call edges — persisted as a git artifact and shared across sessions and developers. Unlike LLM-generated knowledge graphs, our approach requires no inference at index time, produces a reproducible artifact, and enables multi-session reuse through standard git workflows. We introduce the waste ratio (per-turn token growth rate) as a formal benchmark metric and demonstrate through a controlled multi-codebase experiment that pre-computed spatial orientation reduces context accumulation without sacrificing task accuracy.

---

## 7. References

| # | Citation |
|---|----------|
| 1 | Jimenez et al. — SWE-bench: Can Language Models Resolve Real-World GitHub Issues? (2024) |
| 2 | Yang et al. — SWE-agent: Agent-Computer Interfaces Enable Automated Software Engineering. NeurIPS 2024. arXiv:2405.15793 |
| 3 | Ding et al. — CrossCodeEval: A Diverse and Multilingual Benchmark for Cross-File Code Completion. NeurIPS 2023. arXiv:2310.11248 |
| 4 | Prometheus: Long-Horizon Codebase Navigation. arXiv:2507.19942 (2025) |
| 5 | GraphCodeAgent. arXiv:2504.10046 (2025) |
| 6 | LocAgent: Graph-Guided LLM Agents for Software Fault Localization. ACL 2025 |
| 7 | DeusData — Codebase-Memory MCP Server. github.com/DeusData/codebase-memory-mcp (2025) |
| 8 | Theory of Code Space (ToCS). arXiv:2603.00601 (2026) |
| 9 | Codified Context: Infrastructure for AI Coding Agents. arXiv:2602.20478 (2026) |
| 10 | Lessi — I Tracked Every Token My AI Coding Agent Consumed for a Week: 70% Was Waste. DEV Community (2026) |
| 11 | ACON: Optimizing Context Compression for Long-Horizon LLM Agents. arXiv:2510.00615. NeurIPS 2024 |
| 12 | Liu et al. — Lost in the Middle: How Language Models Use Long Contexts. Stanford (2023). arXiv:2307.03172 |
| 13 | Packer et al. — MemGPT: Towards LLMs as Operating Systems (2023). arXiv:2310.08560 |
| 14 | A-Mem: Agentic Memory System. arXiv:2502.12110 (2025) |
| 15 | Mem0: The Memory Layer for Personalized AI. arXiv:2504.19413 (2025) |
| 16 | cAST: Enhancing Code Retrieval via AST-Based Chunking. arXiv:2506.15655 |
| 17 | Roper & Hewitt-Dundas — Knowledge Stocks, Knowledge Flows and Innovation. Research Policy 44(7), 2015 |
| 18 | Smite et al. — Spotify Guilds: How to Succeed With Knowledge Sharing in Large-Scale Agile Organizations. IEEE Software (2019) |
