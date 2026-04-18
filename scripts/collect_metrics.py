#!/usr/bin/env python3
"""
Collects waste ratio, orientation turns, and total tokens from a session JSONL.
Prints a single CSV row: agent,codebase,run,waste_ratio,orientation_turns,total_tokens

Usage:
    python3 scripts/collect_metrics.py <session.jsonl> --agent C --codebase C2 --run 1
"""
import argparse
import json
import sys


def parse_turns(path):
    turns = []
    with open(path) as f:
        for line in f:
            try:
                r = json.loads(line)
            except Exception:
                continue
            if r.get("type") != "assistant":
                continue
            usage = r.get("message", {}).get("usage", {})
            total = (
                usage.get("input_tokens", 0)
                + usage.get("output_tokens", 0)
                + usage.get("cache_creation_input_tokens", 0)
                + usage.get("cache_read_input_tokens", 0)
            )
            if total > 0:
                turns.append(total)
    return turns


def parse_orientation(path):
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
                        return turn
                if c.get("type") == "text":
                    if "## Task 1" in c.get("text", ""):
                        return turn
    return -1


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("session", help="Path to session JSONL file")
    parser.add_argument("--agent", required=True, help="Agent ID: A, B, or C")
    parser.add_argument("--codebase", required=True, help="Codebase ID: C1, C2, or C3")
    parser.add_argument("--run", required=True, help="Run number: 1, 2, or 3")
    args = parser.parse_args()

    turns = parse_turns(args.session)
    if not turns:
        print(f"ERROR: no turns found in {args.session}", file=sys.stderr)
        sys.exit(1)

    baseline = sum(turns[:5]) / min(5, len(turns))
    latest = sum(turns[max(0, len(turns) - 3):]) / min(3, len(turns))
    waste = latest / baseline if baseline > 0 else 1.0
    total = sum(turns)
    orientation = parse_orientation(args.session)

    print("agent,codebase,run,turns,waste_ratio,orientation_turns,total_tokens")
    print(
        f"{args.agent},{args.codebase},{args.run},"
        f"{len(turns)},{waste:.3f},{orientation},{total}"
    )


if __name__ == "__main__":
    main()
