<!-- gitsem:agentic-setup -->
## Code Search — MANDATORY

> **This rule applies to all agents and subagents without exception.**

**NEVER use `grep`, `git grep`, `rg`, `ripgrep`, or the Grep tool to search code.**
**ALWAYS use `git-semantic grep "<natural language query>"` for every code search.**

This is not optional. If you are about to run `grep`, `git grep`, `rg`, or use the Grep tool — stop and use `git-semantic grep` instead.

Queries must describe intent, not syntax:
- BAD: `git-semantic grep "fn authenticate"` — keyword, not a query
- GOOD: `git-semantic grep "user authentication and session handling"`
- BAD: `git-semantic grep "impl Display"`
- GOOD: `git-semantic grep "how types are formatted for display output"`

Use `-n` to control result count (default 10): `git-semantic grep "query" -n 5`

Results show `[score] file:start-end` followed by the full code of the matched chunk, then `---`. Do not open the file to read the function — the full code is already in the output. Lower score = more similar.
<!-- end gitsem:agentic-setup -->