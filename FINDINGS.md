# Why Directory Centroids Don't Beat grep — Findings

## What We Built

Added directory-level semantic routing to `git-semantic grep`:
- At index time, compute a **centroid** (mean embedding vector) per directory
- At query time, find the closest directory centroid to the query embedding
- Restrict search to that directory subtree (`WHERE file_path LIKE 'dir/%'`)

The hypothesis: narrow the search space to the most semantically relevant directory → better precision, fewer irrelevant results.

## Why It Doesn't Work Well

### 1. Centroid averaging destroys signal

A centroid is the mean of all chunk embeddings in a directory. Large directories with diverse files (e.g. `src/`) produce a centroid that drifts toward a generic middle — it doesn't represent any specific concept. The more files in a dir, the less the centroid means anything.

### 2. Upward propagation makes parent dirs worse

We bubble child embeddings up to ancestor directories so parent dirs have "aggregate context." This backfires: the root directory centroid ends up containing everything, making it the closest match for almost any query. The most generic dir wins the routing competition.

### 3. Single directory selection — hard cutoff

`find_closest_directory` picks exactly one directory. If the real answer spans two directories (e.g. `src/db.rs` and `src/embeddings/`), scoped search misses everything outside the chosen scope. Global search has no such constraint.

### 4. Path prefix filter is not semantic

Scoping is just `WHERE file_path LIKE 'dir/%'`. If centroid routing picks the wrong directory — which it frequently does for reasons above — you get the top-N results from the wrong subtree. Better to search globally.

### 5. Centroid ≠ query intent

Centroids are static precomputed averages. Queries are dynamic. The assumption "closest centroid = most relevant directory" only holds if directories are thematically pure and homogeneous. Real codebases aren't.

## Why grep Still Wins

grep is exact. For code search, developers usually know the tokens they're looking for — function names, error messages, type names. Semantic search has an edge on *intent-based* queries ("how does authentication work"), but keyword queries are grep's home turf. The centroid routing adds latency and routing errors without improving recall on the queries where semantic search already had an advantage.

## What Would Actually Help

- **Top-K directory candidates** instead of one — union results from top 3 dirs, rerank globally
- **Weight centroids by inverse chunk count** — small focused dirs get higher confidence than large catch-all dirs
- **Skip scoping for root/large dirs** — fall back to global search when the winning centroid covers >N% of the index
- **Cross-encoder reranking** — run a reranker model over the top-50 global results; this is where most semantic search quality gains come from vs. keyword search
- **Better chunking granularity** — smaller, more focused chunks improve both recall and ranking precision

## Key Insight

The bottleneck isn't search scope — it's result quality. Directory routing is an optimization for speed, not accuracy. Accuracy requires better ranking, not narrower retrieval.
