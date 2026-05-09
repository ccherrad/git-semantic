# Quickstart

Three paths depending on your role. Pick one.

---

## I just joined a team that already uses git-semantic

Someone on your team has already indexed the repo and pushed the `semantic` branch. You just need to pull it locally.

```bash
cargo install gitsem
cd your-repo
git fetch origin semantic
git-semantic hydrate
```

That's it. You now have a local search index. Try it:

```bash
git-semantic map "where does authentication happen"
git-semantic grep "rate limiting logic"
git-semantic health
```

---

## I want to index my repo for the first time

**Step 1 — Install**

```bash
cargo install gitsem
```

**Step 2 — Index**

No API key needed. Gemma runs locally:

```bash
cd your-repo
git-semantic index
```

First run downloads the Gemma model (~500MB, cached at `~/.cache/fastembed`). Subsequent runs are incremental.

![index and hydrate](../demo/index.gif)

**Step 3 — Search**

```bash
git-semantic map "entry point for HTTP requests"
git-semantic grep "connection pool management"
git-semantic health
```

**Step 4 — Share with your team**

```bash
git push origin semantic
```

Everyone else runs:

```bash
git fetch origin semantic
git-semantic hydrate
```

No re-embedding. No API keys. One index, shared across the team.

---

## I want to set up CI so the index stays fresh

See [ci.md](ci.md).

---

## I want to connect it to Claude Code / Cursor / Windsurf

See [mcp.md](mcp.md).
