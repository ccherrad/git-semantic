# Repo Health

`git-semantic health` shows a cohesion/coupling heatmap of your codebase, organized by semantic community (files grouped by what they do, not where they live).

```bash
git-semantic health
```

---

## Reading the output

```
COMMUNITY                        COHESION               FILES  CHUNKS  COUP-OUT  FAN-IN
─────────────────────────────────────────────────────────────────────────────────────────
src/textual (BadIdentifier)      █████████████░░░░░░░  0.63      20     142       149     543
src/textual/widgets (Static)     █████████████░░░░░░░  0.67      20      55        96     416
src/textual/css                  ██████████░░░░░░░░░░  0.49       8     145        12      45
```

**COMMUNITY** — name derived from the centroid file and its dominant symbol. Partial path + class/function name.

**COHESION** — average pairwise embedding similarity of files in the cluster. How semantically related the files actually are.
- Green (≥0.75) — tight, focused cluster
- Yellow (≥0.50) — loosely related, worth watching
- Red (<0.50) — mixed concerns, consider splitting

**FILES / CHUNKS** — size of the community.

**COUP-OUT** — outbound coupling. How many unique symbols this community exports to others. High values mean changes here ripple outward.

**FAN-IN** — how many external call sites depend on this community. The blast radius of a breaking change.
- Green (≤2) — isolated, safe to change
- Yellow (≤6) — moderate dependents
- Red (>6) — high blast radius, change carefully

---

## What to look for

**Red cohesion + high fan-in** — the worst combination. Unrelated things are grouped together AND everything depends on them. Prime candidate for decomposition.

**High fan-in, green cohesion** — stable core module. Don't change the interface; add, don't modify.

**High coup-out** — this community reaches into many others. Either it's a legitimate orchestration layer, or it has too many responsibilities.

**1.00 cohesion, zero coupling** — perfectly isolated utility. Safe to refactor freely.

---

## Drilling into a community

```bash
git-semantic health --community "BadIdentifier"
```

Partial name match — you don't need the full name. Shows:

- All files in the community
- Top 10 communities that depend on it (with the symbols they import)
- Top 10 communities it depends on (with the symbols it imports)

Example output:

```
src/textual (BadIdentifier)
────────────────────────────────────────
  cohesion 0.63   files 20   chunks 142   coup-out 149   fan-in 543

Files (20)
  src/textual/app.py
  src/textual/dom.py
  src/textual/reactive.py
  src/textual/binding.py
  ...

Top dependents (communities importing from this one)
  tests (test_widget_construct)  via Binding, DataTable, NodeList, OptionList...
  docs/examples/widgets (TableApp)  via Binding, DataTable, Option, OptionList...
  ...

Top dependencies (communities this one imports from)
  src/textual/drivers (ParseError)  via Callback, Compose, Enter, Event...
  src/textual (StylesCache)  via Document, Padding, RichLog, Strip...
  ...
```

---

## Workflow for a tech lead

1. Run `git-semantic health` — scan for red fan-in numbers
2. Pick the highest fan-in community that also has low cohesion
3. Run `--community <name>` to see which files are in it and what depends on it
4. Use `git-semantic get <file> --mode outline` on each file to see what's actually in them
5. Identify which files don't belong together — those are the split candidates

The key insight: **fan-in tells you the blast radius, cohesion tells you whether the grouping is justified**. A high fan-in with high cohesion is fine — it's a focused, stable API. A high fan-in with low cohesion is a problem — things that don't belong together are being changed together, and everything breaks when they do.

---

## Enriching with external signals

Health metrics are designed to be extended. A future tool (`git-cognitive`) can inject additional signals — churn rate, cyclomatic complexity, bug density — into the same view. The community structure from `git-semantic` acts as the grouping unit; external tools add the per-community signal columns.
