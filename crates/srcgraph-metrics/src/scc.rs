//! Strongly-connected components — Tarjan via `petgraph::algo::tarjan_scc`,
//! plus condensation DAG and per-SCC status rating.
//!
//! See `DESIGN.md` (Phase 1) at the workspace root.
//!
//! @yah:ticket(R152-T2, "metrics::scc — Tarjan SCC + condensation DAG + rate_scc classifier (ready/partial/entangled)")
//! @yah:assignee(agent:claude)
//! @yah:at(2026-05-12T22:14:25Z)
//! @yah:status(review)
//! @yah:parent(R152)
//! @yah:verify("cargo test -p srcgraph-metrics scc")
//! @yah:handoff("Implemented SccStatus enum (Ready/Partial/Entangled), SccInfo, SccResult, rate_scc classifier, and compute_scc generic over N:ClassNode+E:EdgeKind. Builds condensation DAG alongside SCC list. 5 tests all pass.")

use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, Graph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use srcgraph_core::{ClassNode, EdgeKind};

/// Per-SCC severity classification.
///
/// Thresholds: 1 node → Ready; 2–3 → Partial; ≥4 → Entangled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SccStatus {
    /// Singleton SCC — class has no circular dependency.
    Ready,
    /// 2–3 class cycle — tractable to break with moderate refactoring.
    Partial,
    /// ≥4 class cycle — deeply tangled, needs architectural redesign.
    Entangled,
}

/// Information about one strongly-connected component.
#[derive(Debug, Clone)]
pub struct SccInfo {
    /// Ordinal within `SccResult::components` (also the weight of its DAG node).
    pub id: usize,
    /// Node indices in the original graph belonging to this SCC.
    pub members: Vec<NodeIndex>,
    pub status: SccStatus,
}

/// Output of [`compute_scc`].
pub struct SccResult {
    /// SCCs in reverse topological order (sinks first — matches `tarjan_scc` output).
    pub components: Vec<SccInfo>,
    /// Condensation DAG: each node holds the SCC index into `components`.
    /// An edge `u → v` means the SCC `u` has a dependency on SCC `v`.
    pub dag: DiGraph<usize, ()>,
}

/// Classify an SCC by its member count.
pub fn rate_scc(size: usize) -> SccStatus {
    match size {
        1 => SccStatus::Ready,
        2..=3 => SccStatus::Partial,
        _ => SccStatus::Entangled,
    }
}

/// Compute SCCs and the condensation DAG for `graph`.
///
/// Generic over `N: ClassNode` and `E: EdgeKind` so this works on both
/// [`OwnedGraph`](srcgraph_core::OwnedGraph) (from GraphML) and `kg-store` graph types
/// that implement those traits.
pub fn compute_scc<N, E>(graph: &Graph<N, E>) -> SccResult
where
    N: ClassNode,
    E: EdgeKind,
{
    let raw: Vec<Vec<NodeIndex>> = tarjan_scc(graph);

    // Map every graph node to its SCC index so we can build the condensation.
    let mut node_to_scc: HashMap<NodeIndex, usize> = HashMap::with_capacity(graph.node_count());
    let components: Vec<SccInfo> = raw
        .into_iter()
        .enumerate()
        .map(|(i, members)| {
            for &n in &members {
                node_to_scc.insert(n, i);
            }
            SccInfo {
                id: i,
                status: rate_scc(members.len()),
                members,
            }
        })
        .collect();

    // Build condensation: one DAG node per SCC, one DAG edge per distinct inter-SCC edge.
    let mut dag: DiGraph<usize, ()> = DiGraph::with_capacity(components.len(), 0);
    let dag_nodes: Vec<_> = (0..components.len()).map(|i| dag.add_node(i)).collect();

    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    for e in graph.edge_references() {
        let s = node_to_scc[&e.source()];
        let t = node_to_scc[&e.target()];
        if s != t && seen.insert((s, t)) {
            dag.add_edge(dag_nodes[s], dag_nodes[t], ());
        }
    }

    SccResult { components, dag }
}

#[cfg(test)]
mod tests {
    use super::*;
    use srcgraph_core::{EdgeType, OwnedClassNode, OwnedGraph};
    use petgraph::Graph;

    fn node(id: &str) -> OwnedClassNode {
        OwnedClassNode {
            id: id.to_owned(),
            name: id.split('.').last().unwrap_or(id).to_owned(),
            namespace: "test".to_owned(),
            line_count: 10,
            method_count: 2,
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

    #[test]
    fn scc_linear_dag_all_ready() {
        // A → B → C — linear DAG, no cycles; each class is its own singleton SCC.
        let mut g: OwnedGraph = Graph::new();
        let a = g.add_node(node("A"));
        let b = g.add_node(node("B"));
        let c = g.add_node(node("C"));
        g.add_edge(a, b, EdgeType::MethodCall);
        g.add_edge(b, c, EdgeType::MethodCall);

        let r = compute_scc(&g);

        assert_eq!(r.components.len(), 3);
        assert!(r.components.iter().all(|s| s.status == SccStatus::Ready));
        assert!(r.components.iter().all(|s| s.members.len() == 1));
        // Condensation DAG mirrors the original edges (one per singleton SCC pair).
        assert_eq!(r.dag.node_count(), 3);
        assert_eq!(r.dag.edge_count(), 2);
    }

    #[test]
    fn scc_three_cycle_partial() {
        // A → B → C → A — one 3-node SCC rated Partial.
        let mut g: OwnedGraph = Graph::new();
        let a = g.add_node(node("A"));
        let b = g.add_node(node("B"));
        let c = g.add_node(node("C"));
        g.add_edge(a, b, EdgeType::MethodCall);
        g.add_edge(b, c, EdgeType::MethodCall);
        g.add_edge(c, a, EdgeType::MethodCall);

        let r = compute_scc(&g);

        assert_eq!(r.components.len(), 1, "entire graph is one SCC");
        assert_eq!(r.components[0].status, SccStatus::Partial);
        assert_eq!(r.components[0].members.len(), 3);
        // Condensation is a single node, no edges.
        assert_eq!(r.dag.edge_count(), 0);
    }

    #[test]
    fn scc_large_cycle_entangled() {
        // 5-node cycle — rated Entangled.
        let mut g: OwnedGraph = Graph::new();
        let nodes: Vec<_> = (0..5).map(|i| g.add_node(node(&format!("N{i}")))).collect();
        for i in 0..5 {
            g.add_edge(nodes[i], nodes[(i + 1) % 5], EdgeType::MethodCall);
        }

        let r = compute_scc(&g);

        assert_eq!(r.components.len(), 1);
        assert_eq!(r.components[0].status, SccStatus::Entangled);
        assert_eq!(r.components[0].members.len(), 5);
    }

    #[test]
    fn scc_condensation_dag_shape() {
        // A ↔ B (2-cycle), C → A, D → C.
        // SCCs: {A,B} (Partial), {C} (Ready), {D} (Ready).
        // Condensation edges: D→C, C→{AB} — two edges.
        let mut g: OwnedGraph = Graph::new();
        let a = g.add_node(node("A"));
        let b = g.add_node(node("B"));
        let c = g.add_node(node("C"));
        let d = g.add_node(node("D"));
        g.add_edge(a, b, EdgeType::MethodCall);
        g.add_edge(b, a, EdgeType::MethodCall); // A ↔ B cycle
        g.add_edge(c, a, EdgeType::MethodCall); // C depends on cycle
        g.add_edge(d, c, EdgeType::MethodCall); // D → C

        let r = compute_scc(&g);

        assert_eq!(r.components.len(), 3, "3 SCCs: {{A,B}}, {{C}}, {{D}}");

        let pair = r.components.iter().find(|s| s.members.len() == 2).expect("2-node SCC");
        assert_eq!(pair.status, SccStatus::Partial);

        assert_eq!(r.dag.node_count(), 3);
        assert_eq!(r.dag.edge_count(), 2, "two inter-SCC edges");
    }

    #[test]
    fn rate_scc_thresholds() {
        assert_eq!(rate_scc(1), SccStatus::Ready);
        assert_eq!(rate_scc(2), SccStatus::Partial);
        assert_eq!(rate_scc(3), SccStatus::Partial);
        assert_eq!(rate_scc(4), SccStatus::Entangled);
        assert_eq!(rate_scc(100), SccStatus::Entangled);
    }
}
