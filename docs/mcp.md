# MCP Server Setup

`git-semantic mcp` starts a JSON-RPC server over stdio that exposes `map`, `get`, `grep`, and `health` as tools to any MCP-compatible client.

```bash
git-semantic mcp
```

---

## Claude Code

Add to `.claude/settings.json` in your project (or `~/.claude/settings.json` for all projects):

```json
{
  "mcpServers": {
    "git-semantic": {
      "command": "git-semantic",
      "args": ["mcp"]
    }
  }
}
```

Or use the built-in setup command:

```bash
git-semantic enable claude
```

This installs a `@git-semantic` subagent at `.claude/agents/git-semantic.md` that Claude invokes automatically when navigating code.

---

## Cursor

Add to `.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "git-semantic": {
      "command": "git-semantic",
      "args": ["mcp"]
    }
  }
}
```

---

## Windsurf / Codex / other MCP clients

The server speaks standard MCP (JSON-RPC over stdio). Any client that supports MCP tool calls works. Point the client at:

```
command: git-semantic
args: ["mcp"]
```

---

## Available tools

| Tool | Description |
|---|---|
| `map` | Orient to a task — returns the relevant semantic community with file locations |
| `get` | Read a file by path (with outline/signatures/full modes) or an exact chunk by range |
| `grep` | Hybrid BM25 + semantic search — natural language or exact identifiers |
| `health` | Community heatmap — cohesion, coupling, fan-in per cluster |

---

## Prerequisites

The index must exist before the MCP server can serve queries. Run once:

```bash
git-semantic index    # or: git fetch origin semantic && git-semantic hydrate
```
