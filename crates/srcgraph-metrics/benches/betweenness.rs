//! Wall-clock benchmark for `compute_betweenness` on synthetic random DAGs.
//!
//! Harness-less (`harness = false` in Cargo.toml) so the bench reports plain
//! `ms/iter` lines instead of pulling in criterion. Companion script
//! `benches/networkx_betweenness.py` consumes the same GraphML files this
//! binary writes and runs `nx.betweenness_centrality` for comparison.
//!
//! Usage:
//!   cargo bench -p srcgraph-metrics --bench betweenness
//!   python crates/srcgraph-metrics/benches/networkx_betweenness.py

use std::time::Instant;

use srcgraph_core::{write_graphml, EdgeType, OwnedClassNode, OwnedGraph};
use srcgraph_metrics::betweenness::compute_betweenness;
use petgraph::Graph;

fn make_node(id: usize) -> OwnedClassNode {
    OwnedClassNode {
        id: format!("N{id}"),
        name: format!("N{id}"),
        namespace: "bench".to_owned(),
        line_count: 10,
        method_count: 3,
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

/// xorshift64 — tiny deterministic PRNG so benches are reproducible without a
/// rand dependency.
fn next(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

/// Build an Erdős–Rényi-ish directed graph with `n` nodes and roughly `avg_deg`
/// average out-degree.
fn build_graph(n: usize, avg_deg: usize, seed: u64) -> OwnedGraph {
    let mut g: OwnedGraph = Graph::new();
    let idx: Vec<_> = (0..n).map(|i| g.add_node(make_node(i))).collect();
    let target_edges = (n * avg_deg) as u64;
    let mut s = seed.wrapping_add(1);
    for _ in 0..target_edges {
        let a = (next(&mut s) as usize) % n;
        let b = (next(&mut s) as usize) % n;
        if a != b {
            g.add_edge(idx[a], idx[b], EdgeType::MethodCall);
        }
    }
    g
}

fn bench_one(n: usize, avg_deg: usize) {
    let g = build_graph(n, avg_deg, 0xC0FFEE);
    // Warm-up.
    let _ = compute_betweenness(&g, true);
    let runs = if n >= 2000 { 1 } else if n >= 500 { 3 } else { 5 };

    let mut min_ms = f64::INFINITY;
    for _ in 0..runs {
        let t0 = Instant::now();
        let r = compute_betweenness(&g, true);
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        std::hint::black_box(r);
        if ms < min_ms {
            min_ms = ms;
        }
    }
    println!(
        "n={:>5} edges≈{:>6} avg_deg={:>2}  best={:>9.3} ms  ({} runs)",
        n,
        g.edge_count(),
        avg_deg,
        min_ms,
        runs,
    );

    // Emit GraphML alongside so the Python script can compare on the same
    // graph. Anchored to CARGO_MANIFEST_DIR so the file lands in a predictable
    // place regardless of cargo bench's CWD.
    let dir = format!("{}/../../target/bench-graphs", env!("CARGO_MANIFEST_DIR"));
    let _ = std::fs::create_dir_all(&dir);
    let path = format!("{dir}/bench-graph-n{n}.graphml");
    if let Ok(xml) = write_graphml(&g) {
        let _ = std::fs::write(&path, xml);
    }
}

fn main() {
    println!("# srcgraph-metrics::betweenness — wall-clock bench");
    println!("# {} {} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"), std::env::consts::ARCH);
    for &(n, deg) in &[(100, 4), (500, 4), (1000, 4), (2000, 4), (5000, 4)] {
        bench_one(n, deg);
    }
}
