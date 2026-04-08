# Semantic Search Benchmark — 20 Code Investigation Tasks

## Instructions for the Agent

You are investigating the `git-semantic` Rust codebase (a CLI tool that provides semantic/vector search over Git repositories using embeddings). The source lives in `src/`.

For **each task below**, perform a thorough code investigation and append your findings to a single output file: `BENCHMARK_RESULTS.md`. Each section must start with `## Task N: <title>` and contain:

- The exact search command(s) you ran
- The relevant code you found (file path + line range)
- A clear explanation of how the code works or answers the question
- Any notable design decisions you observe

Work through all 20 tasks sequentially. Do not skip any.

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

## Task 15: `setup` Command Behavior

What exactly does the `setup` (or `agentic-setup`) command do step by step? What files does it create or modify, what content does it inject, and under what conditions does it skip or overwrite existing content?

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
