//! LCOM4 cohesion — connected components on the per-class method-field
//! bipartite graph. LCOM4 = number of components (1 = cohesive).
//!
//! See `DESIGN.md` (Phase 1) at the workspace root.
//!
//! @yah:ticket(R152-T3, "metrics::lcom4 — connected components on per-class method-field bipartite graph")
//! @yah:assignee(agent:claude)
//! @yah:at(2026-05-12T22:14:28Z)
//! @yah:status(review)
//! @yah:parent(R152)
//! @yah:verify("cargo test -p srcgraph-metrics lcom4")
//! @yah:handoff("Implemented LCOM4: build_method_graph (UnGraph from MethodConnectivity), compute_lcom4 (petgraph connected_components, None for empty), compute_lcom4_all (per-class scores generic over ClassNode+EdgeKind; None when methodConnectivity is absent). 6 tests pass; covers cohesive, disjoint, isolated, empty, ghost-endpoint, and mixed-graph cases.")

use petgraph::algo::connected_components;
use petgraph::graph::UnGraph;
use petgraph::Graph;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use srcgraph_core::{ClassNode, EdgeKind, MethodConnectivity};

/// LCOM4 score for a single class.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LcomScore {
    /// `ClassNode::id()` of the class this score belongs to.
    pub class_id: String,
    /// Number of connected components in the method graph.
    /// `None` when the class has no `methodConnectivity` (syntax-only extraction)
    /// or when its method set is empty (LCOM4 is undefined).
    pub lcom4: Option<usize>,
}

/// Build the undirected method graph from a [`MethodConnectivity`] record.
///
/// Node set is `conn.methods`; edge set is `conn.edges`. Edge endpoints that
/// are not listed in `conn.methods` are added as additional nodes (matches the
/// permissive behaviour of `nx.Graph.add_edges_from`).
pub fn build_method_graph(conn: &MethodConnectivity) -> UnGraph<String, ()> {
    let mut g: UnGraph<String, ()> = UnGraph::new_undirected();
    let mut idx: HashMap<&str, _> = HashMap::with_capacity(conn.methods.len());
    for m in &conn.methods {
        let id = g.add_node(m.clone());
        idx.insert(m.as_str(), id);
    }
    for (a, b) in &conn.edges {
        let ia = *idx
            .entry(a.as_str())
            .or_insert_with(|| g.add_node(a.clone()));
        let ib = *idx
            .entry(b.as_str())
            .or_insert_with(|| g.add_node(b.clone()));
        g.add_edge(ia, ib, ());
    }
    g
}

/// Connected-component count on an undirected method graph.
///
/// Returns `None` when the graph is empty — LCOM4 is undefined for a class
/// with no methods, matching the `ValueError` raised by the Python reference
/// implementation.
pub fn compute_lcom4(method_graph: &UnGraph<String, ()>) -> Option<usize> {
    if method_graph.node_count() == 0 {
        return None;
    }
    Some(connected_components(method_graph))
}

/// Compute LCOM4 for every class node in `graph`.
///
/// Classes without a `methodConnectivity` attribute yield `LcomScore { lcom4: None }`
/// so callers can distinguish "no data" from "1 component". Output is in node-index
/// order so it lines up with `graph.node_indices()`.
pub fn compute_lcom4_all<N, E>(graph: &Graph<N, E>) -> Vec<LcomScore>
where
    N: ClassNode,
    E: EdgeKind,
{
    graph
        .node_indices()
        .map(|nx| {
            let node = &graph[nx];
            let lcom4 = node
                .method_connectivity()
                .and_then(|conn| compute_lcom4(&build_method_graph(conn)));
            LcomScore {
                class_id: node.id().to_owned(),
                lcom4,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use srcgraph_core::{EdgeType, OwnedClassNode, OwnedGraph};
    use petgraph::Graph;

    fn class(id: &str, conn: Option<MethodConnectivity>) -> OwnedClassNode {
        OwnedClassNode {
            id: id.to_owned(),
            name: id.split('.').last().unwrap_or(id).to_owned(),
            namespace: "test".to_owned(),
            line_count: 10,
            method_count: conn.as_ref().map_or(0, |c| c.methods.len() as u32),
            halstead_eta1: 0,
            halstead_eta2: 0,
            halstead_n1: 0,
            halstead_n2: 0,
            method_connectivity: conn,
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

    fn conn(methods: &[&str], edges: &[(&str, &str)]) -> MethodConnectivity {
        MethodConnectivity {
            methods: methods.iter().map(|s| s.to_string()).collect(),
            edges: edges
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
        }
    }

    #[test]
    fn lcom4_single_cohesive_component() {
        // foo–bar–baz all chained: one component, cohesive.
        let mg = build_method_graph(&conn(
            &["foo", "bar", "baz"],
            &[("foo", "bar"), ("bar", "baz")],
        ));
        assert_eq!(compute_lcom4(&mg), Some(1));
    }

    #[test]
    fn lcom4_two_disjoint_clusters() {
        // foo–bar and baz–qux — two responsibility clusters.
        let mg = build_method_graph(&conn(
            &["foo", "bar", "baz", "qux"],
            &[("foo", "bar"), ("baz", "qux")],
        ));
        assert_eq!(compute_lcom4(&mg), Some(2));
    }

    #[test]
    fn lcom4_isolated_methods_each_count() {
        // Three methods, no shared fields → three components.
        let mg = build_method_graph(&conn(&["foo", "bar", "baz"], &[]));
        assert_eq!(compute_lcom4(&mg), Some(3));
    }

    #[test]
    fn lcom4_empty_is_undefined() {
        let mg = build_method_graph(&conn(&[], &[]));
        assert_eq!(compute_lcom4(&mg), None);
    }

    #[test]
    fn lcom4_edge_endpoint_outside_methods_list() {
        // Edge references a method not listed in `methods` — petgraph/nx both
        // treat it as an additional node; we mirror that.
        let mg = build_method_graph(&conn(&["foo"], &[("foo", "ghost")]));
        assert_eq!(compute_lcom4(&mg), Some(1));
        assert_eq!(mg.node_count(), 2);
    }

    #[test]
    fn lcom4_all_skips_classes_without_connectivity() {
        let mut g: OwnedGraph = Graph::new();
        let a = g.add_node(class(
            "A",
            Some(conn(&["m1", "m2"], &[("m1", "m2")])),
        ));
        let b = g.add_node(class("B", None));
        let c = g.add_node(class(
            "C",
            Some(conn(&["x", "y", "z"], &[])),
        ));
        g.add_edge(a, b, EdgeType::MethodCall);
        g.add_edge(b, c, EdgeType::Inheritance);

        let scores = compute_lcom4_all(&g);
        assert_eq!(scores.len(), 3);

        let by_id: HashMap<_, _> = scores
            .iter()
            .map(|s| (s.class_id.as_str(), s.lcom4))
            .collect();
        assert_eq!(by_id["A"], Some(1));
        assert_eq!(by_id["B"], None, "no methodConnectivity → None");
        assert_eq!(by_id["C"], Some(3), "three isolated methods");
    }
}
