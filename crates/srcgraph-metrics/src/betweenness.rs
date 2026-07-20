//! Betweenness centrality — Brandes' algorithm. The headline perf target
//! versus networkx's Python-loop O(VE) implementation.
//!
//! Reference: Ulrik Brandes, "A Faster Algorithm for Betweenness Centrality",
//! Journal of Mathematical Sociology 25(2):163-177, 2001.
//!
//! Time complexity: O(V·E) for unweighted graphs. We treat all edges as
//! unweighted (BFS), since the visiting tool's class-dependency graphs don't
//! carry semantically meaningful weights.
//!
//! See `DESIGN.md` (Phase 1) at the workspace root.
//!
//! @yah:ticket(R152-T4, "metrics::betweenness — Brandes' algorithm + perf benchmark vs networkx (headline win)")
//! @yah:assignee(agent:claude)
//! @yah:at(2026-05-12T22:14:30Z)
//! @yah:status(review)
//! @yah:parent(R152)
//! @yah:verify("cargo test -p srcgraph-metrics betweenness")
//! @yah:verify("cargo bench -p srcgraph-metrics --bench betweenness && python3 crates/srcgraph-metrics/benches/networkx_betweenness.py")
//! @yah:handoff("Brandes' O(VE) betweenness implemented generic over N:ClassNode/E:EdgeKind; networkx-default normalization (1/((n-1)(n-2))) for directed graphs. 6 unit tests (path, hub, parallel-split-credit, isolated, empty, 3-cycle). Harness-less bench at benches/betweenness.rs writes synthetic Erdős–Rényi-ish DAGs to target/bench-graphs/, companion Python script benches/networkx_betweenness.py reads the same GraphML and times nx.betweenness_centrality. Result on aarch64: Rust 0.24/7.0/20.7/90/604 ms vs networkx 9.4/207/898/3975/28771 ms for n=100/500/1000/2000/5000 — roughly 30–48× speedup, growing with n. Headline win confirmed.")

use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Directed;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

use srcgraph_core::{ClassNode, EdgeKind};

/// Per-node betweenness score, parallel to the input graph's node indices.
///
/// `scores[i]` is the centrality of `NodeIndex::new(i)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BetweennessResult {
    pub scores: Vec<f64>,
    /// Whether `scores` were normalized to `[0, 1]` (matches networkx's default).
    pub normalized: bool,
}

impl BetweennessResult {
    pub fn get(&self, n: NodeIndex) -> f64 {
        self.scores[n.index()]
    }
}

/// Compute betweenness centrality for every node in `graph`.
///
/// `normalized=true` matches networkx's default behavior: scores are scaled by
/// `1 / ((n-1)·(n-2))` for directed graphs with `n ≥ 3`. For `n < 3` scores
/// are returned unscaled (would divide by zero).
///
/// Treats the graph as **directed** and unweighted. To run on an undirected
/// view of the same data, add reverse edges before calling.
pub fn compute_betweenness<N, E>(
    graph: &Graph<N, E, Directed>,
    normalized: bool,
) -> BetweennessResult
where
    N: ClassNode,
    E: EdgeKind,
{
    let n = graph.node_count();
    let mut cb = vec![0.0_f64; n];

    if n == 0 {
        return BetweennessResult { scores: cb, normalized: false };
    }

    // Precompute outgoing-neighbor lists once: avoids repeated graph-edge walks
    // inside the per-source BFS hot loop.
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in graph.edge_references() {
        out[e.source().index()].push(e.target().index());
    }

    let mut stack: Vec<usize> = Vec::with_capacity(n);
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut sigma = vec![0.0_f64; n];
    let mut dist = vec![-1_i64; n];
    let mut delta = vec![0.0_f64; n];
    let mut queue: VecDeque<usize> = VecDeque::with_capacity(n);

    for s in 0..n {
        stack.clear();
        for p in preds.iter_mut() {
            p.clear();
        }
        for x in sigma.iter_mut() {
            *x = 0.0;
        }
        for d in dist.iter_mut() {
            *d = -1;
        }
        for d in delta.iter_mut() {
            *d = 0.0;
        }
        queue.clear();

        sigma[s] = 1.0;
        dist[s] = 0;
        queue.push_back(s);

        while let Some(v) = queue.pop_front() {
            stack.push(v);
            for &w in &out[v] {
                if dist[w] < 0 {
                    dist[w] = dist[v] + 1;
                    queue.push_back(w);
                }
                if dist[w] == dist[v] + 1 {
                    sigma[w] += sigma[v];
                    preds[w].push(v);
                }
            }
        }

        while let Some(w) = stack.pop() {
            let coeff = (1.0 + delta[w]) / sigma[w];
            for &v in &preds[w] {
                delta[v] += sigma[v] * coeff;
            }
            if w != s {
                cb[w] += delta[w];
            }
        }
    }

    if normalized && n > 2 {
        let scale = 1.0 / ((n - 1) as f64 * (n - 2) as f64);
        for x in cb.iter_mut() {
            *x *= scale;
        }
        return BetweennessResult { scores: cb, normalized: true };
    }

    BetweennessResult { scores: cb, normalized: false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use srcgraph_core::{EdgeType, OwnedClassNode, OwnedGraph};
    use petgraph::Graph;

    fn node(id: &str) -> OwnedClassNode {
        OwnedClassNode {
            id: id.to_owned(),
            name: id.to_owned(),
            namespace: "test".to_owned(),
            line_count: 1,
            method_count: 1,
            halstead_eta1: 0,
            halstead_eta2: 0,
            halstead_n1: 0,
            halstead_n2: 0,
            method_connectivity: None,
            method_fingerprints: None,
            method_tokens: None,
            call_sequences: None,
            cyclomatic_complexity: None,
            path_conditions: None,
            invariants: None,
            error_messages: None,
            magic_numbers: None,
            dead_code: None,
            tenant_branches: None,
            state_transitions: None,
        }
    }

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn betweenness_path_graph_unnormalized() {
        // A → B → C → D — directed path.
        //   pairs through B: (A,C), (A,D)  → 2
        //   pairs through C: (A,D), (B,D)  → 2
        let mut g: OwnedGraph = Graph::new();
        let a = g.add_node(node("A"));
        let b = g.add_node(node("B"));
        let c = g.add_node(node("C"));
        let d = g.add_node(node("D"));
        g.add_edge(a, b, EdgeType::MethodCall);
        g.add_edge(b, c, EdgeType::MethodCall);
        g.add_edge(c, d, EdgeType::MethodCall);

        let r = compute_betweenness(&g, false);
        assert!(approx(r.get(a), 0.0));
        assert!(approx(r.get(b), 2.0), "B={}", r.get(b));
        assert!(approx(r.get(c), 2.0), "C={}", r.get(c));
        assert!(approx(r.get(d), 0.0));
        assert!(!r.normalized);
    }

    #[test]
    fn betweenness_hub_normalized() {
        // Bidirectional star: A↔B, A↔C, A↔D. Every leaf-pair shortest path
        // routes through A. Raw σ_A = 6 (ordered pairs); n=4 ⇒ scale=1/6 ⇒ 1.0.
        let mut g: OwnedGraph = Graph::new();
        let a = g.add_node(node("A"));
        let leaves: Vec<_> = ["B", "C", "D"].iter().map(|l| g.add_node(node(l))).collect();
        for &l in &leaves {
            g.add_edge(a, l, EdgeType::MethodCall);
            g.add_edge(l, a, EdgeType::MethodCall);
        }

        let r = compute_betweenness(&g, true);
        assert!(r.normalized);
        assert!(approx(r.get(a), 1.0), "hub={}", r.get(a));
        for &l in &leaves {
            assert!(approx(r.get(l), 0.0), "leaf={}", r.get(l));
        }
    }

    #[test]
    fn betweenness_parallel_paths_split_credit() {
        // A → {B,C} → D — two equal-length paths A↦D.
        // For source A, σ_D = 2; each intermediate gets 1/2.
        let mut g: OwnedGraph = Graph::new();
        let a = g.add_node(node("A"));
        let b = g.add_node(node("B"));
        let c = g.add_node(node("C"));
        let d = g.add_node(node("D"));
        g.add_edge(a, b, EdgeType::MethodCall);
        g.add_edge(a, c, EdgeType::MethodCall);
        g.add_edge(b, d, EdgeType::MethodCall);
        g.add_edge(c, d, EdgeType::MethodCall);

        let r = compute_betweenness(&g, false);
        assert!(approx(r.get(a), 0.0));
        assert!(approx(r.get(b), 0.5), "B={}", r.get(b));
        assert!(approx(r.get(c), 0.5), "C={}", r.get(c));
        assert!(approx(r.get(d), 0.0));
    }

    #[test]
    fn betweenness_isolated_nodes_zero() {
        let mut g: OwnedGraph = Graph::new();
        let a = g.add_node(node("A"));
        let b = g.add_node(node("B"));
        let c = g.add_node(node("C"));
        let r = compute_betweenness(&g, true);
        for nx in [a, b, c] {
            assert!(approx(r.get(nx), 0.0));
        }
    }

    #[test]
    fn betweenness_empty_graph() {
        let g: OwnedGraph = Graph::new();
        let r = compute_betweenness(&g, true);
        assert!(r.scores.is_empty());
    }

    #[test]
    fn betweenness_three_cycle_symmetric() {
        // A→B→C→A. Each node lies on exactly one shortest path of the
        // remaining pair (directed 2-hop), so all three scores equal 1.
        let mut g: OwnedGraph = Graph::new();
        let a = g.add_node(node("A"));
        let b = g.add_node(node("B"));
        let c = g.add_node(node("C"));
        g.add_edge(a, b, EdgeType::MethodCall);
        g.add_edge(b, c, EdgeType::MethodCall);
        g.add_edge(c, a, EdgeType::MethodCall);

        let r = compute_betweenness(&g, false);
        assert!(approx(r.get(a), 1.0), "A={}", r.get(a));
        assert!(approx(r.get(b), 1.0), "B={}", r.get(b));
        assert!(approx(r.get(c), 1.0), "C={}", r.get(c));
    }
}
