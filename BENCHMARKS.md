# Benchmarks

Results from `git-semantic benchmark` on real codebases. Run it on your own repo to get equivalent numbers.

---

## [Textual](https://github.com/Textualize/textual) — 988 Python files

**Token savings by read mode**

| mode | tokens | vs raw |
|------|--------|--------|
| raw | 1.1M | — |
| full (chunks) | 1.1M | 4.3% |
| signatures | 152K | 86.4% |
| outline | 41K | **96.3%** |

**Session simulation** (10 files navigated, $3/1M tokens)

| scenario | tokens | cost | savings |
|----------|--------|------|---------|
| raw (read whole files) | 11K | $0.034 | — |
| grep only | 8K | $0.024 | 28.8% |
| map + outline + get | 3K | $0.009 | **72.3%** |
| map + signatures + get | 4K | $0.013 | 62.4% |

**Navigation comparison** (10 sampled subsystem queries)

| strategy | avg tokens/query | precision |
|----------|-----------------|-----------|
| grep only (top 5) | 377 | 40% |
| map + outline + get | 2K | **100%** |
| map + signatures + get | 2K | **100%** |

Precision = top result belongs to the correct subsystem.

The session simulation assumes one query, fixed chunk count. Real agents don't work that way — Claude Code reads however many results look relevant (typically 2-3, sometimes more) and retries if nothing fits. The token numbers are a lower bound, not the full picture.

What the simulation doesn't capture: at 40% precision, 6 in 10 grep queries land in the wrong subsystem. The agent reads wrong chunks, backtracks, searches again — each retry compounds context. map + outline always lands on the first try, so the end-to-end cost is lower even though the per-query token count is higher. Precision is the metric that matters; the token counts are illustrative.
