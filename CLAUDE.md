<!-- gitsem:agentic-setup -->
## Code Search

Use `gitsem grep "<natural language query>"` instead of `git grep` or `grep` for all code searches.

Queries must describe intent, not syntax:
- BAD: `gitsem grep "fn authenticate"` — this is a keyword, not a query
- GOOD: `gitsem grep "user authentication and session handling"`
- BAD: `gitsem grep "impl Display"`
- GOOD: `gitsem grep "how types are formatted for display output"`

Use `-n` to control result count (default 10): `gitsem grep "query" -n 5`

Results show `[score] file:start-end` followed by a preview line. Lower score = more similar.
<!-- end gitsem:agentic-setup -->