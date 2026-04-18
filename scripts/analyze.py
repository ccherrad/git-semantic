#!/usr/bin/env python3
"""
Statistical analysis of benchmark results.
Reads a CSV with columns: agent,codebase,run,turns,waste_ratio,orientation_turns,total_tokens
Outputs: means, SDs, Wilcoxon tests, Cohen's d.

Usage:
    python3 scripts/analyze.py results.csv
"""
import argparse
import csv
import math
import sys
from collections import defaultdict


def mean(xs):
    return sum(xs) / len(xs) if xs else 0.0


def sd(xs):
    if len(xs) < 2:
        return 0.0
    m = mean(xs)
    return math.sqrt(sum((x - m) ** 2 for x in xs) / (len(xs) - 1))


def cohens_d(a, b):
    pooled_sd = math.sqrt((sd(a) ** 2 + sd(b) ** 2) / 2)
    if pooled_sd == 0:
        return 0.0
    return (mean(a) - mean(b)) / pooled_sd


def wilcoxon(x, y):
    """
    Wilcoxon signed-rank test (paired).
    Returns (W, p_approx) — p is approximated via normal distribution for n >= 10.
    For n < 10, returns exact W and None for p.
    """
    diffs = [xi - yi for xi, yi in zip(x, y) if xi != yi]
    n = len(diffs)
    if n == 0:
        return 0, 1.0

    abs_diffs = sorted(enumerate(abs(d) for d in diffs), key=lambda x: x[1])
    ranks = {}
    i = 0
    while i < n:
        j = i
        while j < n and abs_diffs[j][1] == abs_diffs[i][1]:
            j += 1
        rank = (i + 1 + j) / 2
        for k in range(i, j):
            ranks[abs_diffs[k][0]] = rank
        i = j

    w_plus = sum(ranks[i] for i, d in enumerate(diffs) if d > 0)
    w_minus = sum(ranks[i] for i, d in enumerate(diffs) if d < 0)
    w = min(w_plus, w_minus)

    if n < 10:
        return w, None

    mu = n * (n + 1) / 4
    sigma = math.sqrt(n * (n + 1) * (2 * n + 1) / 24)
    z = (w - mu) / sigma if sigma > 0 else 0
    p = 2 * (1 - _norm_cdf(abs(z)))
    return w, p


def _norm_cdf(x):
    return (1 + math.erf(x / math.sqrt(2))) / 2


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("csv_file", help="CSV results file")
    args = parser.parse_args()

    data = defaultdict(lambda: defaultdict(list))

    with open(args.csv_file) as f:
        reader = csv.DictReader(f)
        for row in reader:
            agent = row["agent"]
            codebase = row["codebase"]
            data[agent][codebase].append({
                "waste_ratio": float(row["waste_ratio"]),
                "orientation_turns": int(row["orientation_turns"]),
                "total_tokens": int(row["total_tokens"]),
            })

    codebases = sorted({cb for agent_data in data.values() for cb in agent_data})
    agents = ["A", "B", "C"]

    print("=" * 60)
    print("WASTE RATIO — mean ± SD")
    print("=" * 60)
    print(f"{'Codebase':<12}", end="")
    for agent in agents:
        print(f"  Agent {agent:<12}", end="")
    print()
    print("-" * 60)

    for cb in codebases + ["ALL"]:
        print(f"{cb:<12}", end="")
        for agent in agents:
            if cb == "ALL":
                vals = [r["waste_ratio"] for runs in data[agent].values() for r in runs]
            else:
                vals = [r["waste_ratio"] for r in data[agent].get(cb, [])]
            if vals:
                print(f"  {mean(vals):.3f} ± {sd(vals):.3f}", end="")
            else:
                print(f"  {'—':<16}", end="")
        print()

    print()
    print("=" * 60)
    print("STATISTICAL TESTS (Wilcoxon signed-rank, paired by codebase+run)")
    print("=" * 60)

    for metric in ["waste_ratio", "orientation_turns"]:
        print(f"\nMetric: {metric}")
        print(f"{'Comparison':<12} {'W':>8} {'p':>10} {'Cohen d':>10} {'Sig':>6}")
        print("-" * 50)
        for comp in [("A", "C"), ("B", "C")]:
            a_label, c_label = comp
            a_vals, c_vals = [], []
            for cb in codebases:
                a_runs = [r[metric] for r in data[a_label].get(cb, [])]
                c_runs = [r[metric] for r in data[c_label].get(cb, [])]
                n = min(len(a_runs), len(c_runs))
                a_vals.extend(a_runs[:n])
                c_vals.extend(c_runs[:n])

            if len(a_vals) < 2:
                print(f"{a_label} vs {c_label:<8} {'insufficient data':>30}")
                continue

            w, p = wilcoxon(a_vals, c_vals)
            d = cohens_d(a_vals, c_vals)
            p_str = f"{p:.4f}" if p is not None else "n<10"
            sig = "✓" if (p is not None and p < 0.05) else "✗"
            print(f"{a_label} vs {c_label:<8} {w:>8.1f} {p_str:>10} {d:>10.3f} {sig:>6}")


if __name__ == "__main__":
    main()
