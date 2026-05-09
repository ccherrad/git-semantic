# CI Setup

Keep the semantic index fresh automatically. One push per merge, the whole team benefits.

---

## GitHub Actions

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
        run: git-semantic index

      - name: Push semantic branch
        run: |
          git config user.name "github-actions[bot]"
          git config user.email "github-actions[bot]@users.noreply.github.com"
          git push origin semantic --force
```

No API key needed — Gemma runs locally. The model is downloaded on first run (~500MB); subsequent runs use the cached model.

If you prefer OpenAI embeddings:

```yaml
      - name: Index codebase
        env:
          OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
        run: |
          git-semantic config provider openai
          git-semantic index
```

---

## After the push

Team members pull the updated index with:

```bash
git fetch origin semantic
git-semantic hydrate
```

Or add this to a post-merge git hook (`.git/hooks/post-merge`):

```bash
#!/bin/sh
git fetch origin semantic 2>/dev/null
git-semantic hydrate 2>/dev/null
```

---

## Resetting the index

If you need to start fresh (e.g. after changing the embedding provider):

```bash
# Local reset
git branch -D semantic
rm .git/semantic.db
git-semantic index

# Push reset to origin
git push origin --delete semantic
git push origin semantic
```
