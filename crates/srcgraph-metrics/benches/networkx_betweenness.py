#!/usr/bin/env python3
"""Time networkx.betweenness_centrality on the GraphML files written by
``cargo bench --bench betweenness``.

Usage::

    cargo bench -p srcgraph-metrics --bench betweenness
    python crates/srcgraph-metrics/benches/networkx_betweenness.py

Reads ``target/bench-graph-n*.graphml`` (relative to the srcgraph workspace
root) so the two timings cover the exact same graphs.
"""
from __future__ import annotations

import glob
import os
import sys
import time

try:
    import networkx as nx
except ImportError:
    print("networkx not installed — `pip install networkx` to compare.", file=sys.stderr)
    sys.exit(1)


def find_workspace_root() -> str:
    # Script lives at crates/srcgraph-metrics/benches/, so the workspace root
    # is three levels up. Confirm by checking for the workspace Cargo.toml.
    here = os.path.abspath(os.path.dirname(__file__))
    root = os.path.abspath(os.path.join(here, "..", "..", ".."))
    return root


def main() -> int:
    root = find_workspace_root()
    pattern = os.path.join(root, "target", "bench-graphs", "bench-graph-n*.graphml")
    paths = sorted(glob.glob(pattern), key=lambda p: int(p.split("-n")[-1].split(".")[0]))
    if not paths:
        print(f"no GraphML files found at {pattern}", file=sys.stderr)
        print("run `cargo bench -p srcgraph-metrics --bench betweenness` first.", file=sys.stderr)
        return 1

    print(f"# networkx {nx.__version__} — betweenness_centrality wall-clock")
    for p in paths:
        g = nx.read_graphml(p)
        n = g.number_of_nodes()
        e = g.number_of_edges()
        runs = 1 if n >= 2000 else (3 if n >= 500 else 5)
        best = float("inf")
        for _ in range(runs):
            t0 = time.perf_counter()
            nx.betweenness_centrality(g, normalized=True, endpoints=False)
            ms = (time.perf_counter() - t0) * 1e3
            if ms < best:
                best = ms
        print(f"n={n:>5} edges={e:>6}  best={best:>9.3f} ms  ({runs} runs)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
