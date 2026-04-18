#!/usr/bin/env python3
"""
Orientation cost parser.
Detects the first turn at which the agent writes a Task 1 answer.

Usage:
    python3 scripts/orientation.py ~/.claude/projects/<dir>/<SESSION_ID>.jsonl
"""
import json
import sys


def main():
    if len(sys.argv) < 2:
        print("Usage: orientation.py <session.jsonl>")
        sys.exit(1)

    path = sys.argv[1]
    turn = 0

    with open(path) as f:
        for line in f:
            try:
                r = json.loads(line)
            except Exception:
                continue
            if r.get("type") != "assistant":
                continue
            turn += 1
            content = r.get("message", {}).get("content", [])
            for c in content:
                if not isinstance(c, dict):
                    continue
                if c.get("type") == "tool_use":
                    inp = str(c.get("input", {}))
                    if "BENCHMARK_RESULTS" in inp and (
                        "Task 1" in inp or "## Task" in inp
                    ):
                        print(f"orientation_turns={turn}")
                        return
                if c.get("type") == "text":
                    if "## Task 1" in c.get("text", ""):
                        print(f"orientation_turns={turn}")
                        return

    print(
        f"orientation_turns=NOT_DETECTED  # checked {turn} turns — verify manually"
    )


if __name__ == "__main__":
    main()
