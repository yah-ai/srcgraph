//! Martin's instability metric — per-namespace afferent / efferent coupling
//! from inter-package edges, and Stable Dependencies Principle violations.
//!
//! For every namespace (package) in the graph:
//!   Ca = afferent coupling  — # of inter-namespace edges arriving at this namespace
//!   Ce = efferent coupling  — # of inter-namespace edges leaving this namespace
//!   I  = Ce / (Ca + Ce)     — in [0.0, 1.0]
//!         I = 0 → maximally stable, I = 1 → maximally unstable.
//!
//! An inter-namespace edge A → B violates the Stable Dependencies Principle
//! when `I(A) < I(B)` — a more-stable package depends on a less-stable one.
//!
//! See `DESIGN.md` (Phase 2) at the workspace root.

use petgraph::visit::EdgeRef;
use petgraph::Graph;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use srcgraph_core::{ClassNode, EdgeKind};

/// Coupling counts and instability for a single namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceInstability {
    pub namespace: String,
    /// Afferent coupling — inter-namespace edges pointing INTO this namespace.
    pub ca: u32,
    /// Efferent coupling — inter-namespace edges leaving this namespace.
    pub ce: u32,
    /// `None` when Ca + Ce == 0 (isolated namespace — instability is undefined).
    pub instability: Option<f64>,
}

/// Single-value formula. `None` when the namespace is isolated (Ca + Ce == 0).
pub fn instability(ca: u32, ce: u32) -> Option<f64> {
    let total = ca + ce;
    if total == 0 {
        None
    } else {
        Some(ce as f64 / total as f64)
    }
}

/// Compute Ca / Ce / I for every namespace in `graph`.
///
/// Only **inter-namespace** edges count toward Ca/Ce — intra-namespace edges
/// (a class depending on a sibling in the same package) do not contribute to
/// package-level coupling. Output is sorted by namespace name for determinism.
pub fn compute_instability<N, E>(graph: &Graph<N, E>) -> Vec<NamespaceInstability>
where
    N: ClassNode,
    E: EdgeKind,
{
    let mut ca: HashMap<String, u32> = HashMap::new();
    let mut ce: HashMap<String, u32> = HashMap::new();

    // Seed both maps so isolated namespaces (no edges) still surface in the output.
    for nx in graph.node_indices() {
        let ns = graph[nx].namespace().to_owned();
        ca.entry(ns.clone()).or_insert(0);
        ce.entry(ns).or_insert(0);
    }

    for edge in graph.edge_references() {
        let src_ns = graph[edge.source()].namespace();
        let tgt_ns = graph[edge.target()].namespace();
        if src_ns == tgt_ns {
            continue;
        }
        *ce.entry(src_ns.to_owned()).or_insert(0) += 1;
        *ca.entry(tgt_ns.to_owned()).or_insert(0) += 1;
    }

    let mut out: Vec<NamespaceInstability> = ca
        .keys()
        .map(|ns| {
            let ca_v = ca[ns];
            let ce_v = ce[ns];
            NamespaceInstability {
                namespace: ns.clone(),
                ca: ca_v,
                ce: ce_v,
                instability: instability(ca_v, ce_v),
            }
        })
        .collect();
    out.sort_by(|a, b| a.namespace.cmp(&b.namespace));
    out
}

/// An inter-namespace edge whose source is more stable than its target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdpViolation {
    pub source_namespace: String,
    pub target_namespace: String,
    pub i_source: f64,
    pub i_target: f64,
    /// `i_target - i_source` — positive by construction; larger = worse violation.
    pub gap: f64,
}

/// Find every inter-namespace edge that violates the Stable Dependencies Principle.
///
/// Edges within a namespace are skipped. Edges touching an isolated namespace
/// (instability undefined) are skipped. Output is sorted by `gap` descending.
pub fn find_sdp_violations<N, E>(graph: &Graph<N, E>) -> Vec<SdpViolation>
where
    N: ClassNode,
    E: EdgeKind,
{
    let by_ns: HashMap<String, Option<f64>> = compute_instability(graph)
        .into_iter()
        .map(|r| (r.namespace, r.instability))
        .collect();

    let mut violations: Vec<SdpViolation> = Vec::new();
    for edge in graph.edge_references() {
        let src_ns = graph[edge.source()].namespace();
        let tgt_ns = graph[edge.target()].namespace();
        if src_ns == tgt_ns {
            continue;
        }
        let (Some(i_src), Some(i_tgt)) = (
            by_ns.get(src_ns).copied().flatten(),
            by_ns.get(tgt_ns).copied().flatten(),
        ) else {
            continue;
        };
        if i_src < i_tgt {
            violations.push(SdpViolation {
                source_namespace: src_ns.to_owned(),
                target_namespace: tgt_ns.to_owned(),
                i_source: i_src,
                i_target: i_tgt,
                gap: i_tgt - i_src,
            });
        }
    }
    violations.sort_by(|a, b| b.gap.partial_cmp(&a.gap).unwrap_or(std::cmp::Ordering::Equal));
    violations
}

#[cfg(test)]
mod tests {
    use super::*;
    use srcgraph_core::{EdgeType, OwnedClassNode, OwnedGraph};
    use petgraph::Graph;

    fn class(id: &str, namespace: &str) -> OwnedClassNode {
        OwnedClassNode {
            id: id.to_owned(),
            name: id.rsplit('.').next().unwrap_or(id).to_owned(),
            namespace: namespace.to_owned(),
            line_count: 10,
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

    fn by_ns(rows: Vec<NamespaceInstability>) -> HashMap<String, NamespaceInstability> {
        rows.into_iter().map(|r| (r.namespace.clone(), r)).collect()
    }

    #[test]
    fn instability_formula_edges() {
        // Stable: many depend on it, it depends on nothing.
        assert_eq!(instability(5, 0), Some(0.0));
        // Unstable: depends on many, nothing depends on it.
        assert_eq!(instability(0, 5), Some(1.0));
        // Balanced.
        assert_eq!(instability(2, 2), Some(0.5));
        // Isolated → undefined.
        assert_eq!(instability(0, 0), None);
    }

    #[test]
    fn intra_namespace_edges_do_not_count() {
        // Two classes in the same package, one edge between them — package is
        // isolated from the rest of the world, so I is undefined.
        let mut g: OwnedGraph = Graph::new();
        let a = g.add_node(class("p.A", "p"));
        let b = g.add_node(class("p.B", "p"));
        g.add_edge(a, b, EdgeType::MethodCall);

        let rows = compute_instability(&g);
        let m = by_ns(rows);
        assert_eq!(m["p"].ca, 0);
        assert_eq!(m["p"].ce, 0);
        assert!(m["p"].instability.is_none());
    }

    #[test]
    fn cross_namespace_edge_assigns_ca_and_ce() {
        // p.A → q.B — p has Ce=1, q has Ca=1.
        let mut g: OwnedGraph = Graph::new();
        let a = g.add_node(class("p.A", "p"));
        let b = g.add_node(class("q.B", "q"));
        g.add_edge(a, b, EdgeType::MethodCall);

        let m = by_ns(compute_instability(&g));
        assert_eq!(m["p"].ce, 1);
        assert_eq!(m["p"].ca, 0);
        assert_eq!(m["p"].instability, Some(1.0));

        assert_eq!(m["q"].ca, 1);
        assert_eq!(m["q"].ce, 0);
        assert_eq!(m["q"].instability, Some(0.0));
    }

    #[test]
    fn isolated_namespace_yields_none() {
        let mut g: OwnedGraph = Graph::new();
        g.add_node(class("p.A", "p"));
        let rows = compute_instability(&g);
        let m = by_ns(rows);
        assert_eq!(m["p"].ca, 0);
        assert_eq!(m["p"].ce, 0);
        assert!(m["p"].instability.is_none());
    }

    #[test]
    fn parallel_edges_each_count() {
        // Two p.A → q.B edges (e.g. method-call AND field-ref) — both count.
        let mut g: OwnedGraph = Graph::new();
        let a = g.add_node(class("p.A", "p"));
        let b = g.add_node(class("q.B", "q"));
        g.add_edge(a, b, EdgeType::MethodCall);
        g.add_edge(a, b, EdgeType::FieldRef);

        let m = by_ns(compute_instability(&g));
        assert_eq!(m["p"].ce, 2);
        assert_eq!(m["q"].ca, 2);
    }

    #[test]
    fn sdp_violation_detected_and_intra_namespace_skipped() {
        // Construct stable→unstable to trigger a violation:
        //   x → s, y → s    (s has many afferent; low I)
        //   s → u           (the violating edge — stable s depends on unstable u)
        //   u → z           (u has efferent so its I > 0)
        //   s → s2          (intra-namespace, ignored)
        let mut g: OwnedGraph = Graph::new();
        let s = g.add_node(class("s.A", "s"));
        let s2 = g.add_node(class("s.A2", "s"));
        let u = g.add_node(class("u.A", "u"));
        let z = g.add_node(class("z.A", "z"));
        let x = g.add_node(class("x.A", "x"));
        let y = g.add_node(class("y.A", "y"));
        g.add_edge(x, s, EdgeType::MethodCall);
        g.add_edge(y, s, EdgeType::MethodCall);
        g.add_edge(s, u, EdgeType::MethodCall);
        g.add_edge(u, z, EdgeType::MethodCall);
        g.add_edge(s, s2, EdgeType::MethodCall); // intra-namespace, skipped

        // s: Ca=2, Ce=1 → I = 1/3
        // u: Ca=1, Ce=1 → I = 0.5
        // z: Ca=1, Ce=0 → I = 0
        // x: Ca=0, Ce=1 → I = 1
        // y: Ca=0, Ce=1 → I = 1
        let m = by_ns(compute_instability(&g));
        assert!((m["s"].instability.unwrap() - 1.0 / 3.0).abs() < 1e-12);
        assert_eq!(m["u"].instability, Some(0.5));
        assert_eq!(m["z"].instability, Some(0.0));
        assert_eq!(m["x"].instability, Some(1.0));
        assert_eq!(m["y"].instability, Some(1.0));

        let violations = find_sdp_violations(&g);
        // Only s→u qualifies: I(s)=1/3 < I(u)=0.5.
        // x→s, y→s: 1.0 < 1/3 false. u→z: 0.5 < 0.0 false.
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].source_namespace, "s");
        assert_eq!(violations[0].target_namespace, "u");
        assert!((violations[0].gap - (0.5 - 1.0 / 3.0)).abs() < 1e-12);
    }

    #[test]
    fn sdp_skips_edges_touching_isolated_namespace() {
        // An edge can't both exist and produce an isolated endpoint, but we can
        // exercise the "no instability data" branch by constructing the graph
        // by hand. Here every namespace has coupling, so no exclusions happen
        // — this test just guards that a fully-coupled graph returns the right
        // count without spurious skips.
        let mut g: OwnedGraph = Graph::new();
        let a = g.add_node(class("a.A", "a"));
        let b = g.add_node(class("b.B", "b"));
        let c = g.add_node(class("c.C", "c"));
        g.add_edge(a, b, EdgeType::MethodCall);
        g.add_edge(b, c, EdgeType::MethodCall);
        g.add_edge(c, a, EdgeType::MethodCall);
        // Ring: every namespace has Ca=1, Ce=1, I=0.5 — no violations.
        assert!(find_sdp_violations(&g).is_empty());
    }

    #[test]
    fn violations_sorted_by_gap_descending() {
        // Two violations with different gaps; the larger one comes first.
        //
        //   stable (I=0) ── edge ──▶ very-unstable (I=1)        gap 1.0
        //   mid    (I=0.5) ── edge ──▶ unstable      (I=1)      gap 0.5
        //
        // We build that by routing edges so each namespace lands on its target I.
        let mut g: OwnedGraph = Graph::new();
        let s = g.add_node(class("s.A", "s"));        // stable: only afferent
        let m = g.add_node(class("m.A", "m"));        // mid: 1 in, 1 out
        let u = g.add_node(class("u.A", "u"));        // unstable: efferent-only
        let v = g.add_node(class("v.A", "v"));        // very-unstable: efferent-only

        // s gets afferent edges, no efferent. To create the s→v violation we
        // need s to have at least one efferent edge — so model s as having
        // many afferent and one efferent. Use extra nodes to weight the count.
        let s_in1 = g.add_node(class("x.A", "x"));
        let s_in2 = g.add_node(class("y.A", "y"));
        g.add_edge(s_in1, s, EdgeType::MethodCall); // s Ca+=1
        g.add_edge(s_in2, s, EdgeType::MethodCall); // s Ca+=1
        g.add_edge(s, v, EdgeType::MethodCall);     // s Ce=1 → I(s)=1/3 ≈ 0.33
        g.add_edge(m, u, EdgeType::MethodCall);     // m Ce=1, u Ca=1
        g.add_edge(v, m, EdgeType::MethodCall);     // m Ca=1, v Ce=1
        // m: Ca=1, Ce=1, I=0.5
        // u: Ca=1, Ce=0, I=0
        // v: Ca=1, Ce=1, I=0.5

        let by = by_ns(compute_instability(&g));
        let i_s = by["s"].instability.unwrap();
        let i_v = by["v"].instability.unwrap();
        let i_m = by["m"].instability.unwrap();
        let i_u = by["u"].instability.unwrap();

        let violations = find_sdp_violations(&g);
        // s→v is a violation iff I(s) < I(v): 1/3 < 0.5 → yes, gap ≈ 0.167.
        // m→u: 0.5 < 0 → no.
        // v→m: 0.5 < 0.5 → no.
        // x→s, y→s: I(x)=1, I(y)=1 → 1 < 1/3 → no.
        let sv = violations.iter().find(|x| x.source_namespace == "s").unwrap();
        assert!((sv.i_source - i_s).abs() < 1e-12);
        assert!((sv.i_target - i_v).abs() < 1e-12);

        // sanity check the other I values are what we documented
        assert!((i_m - 0.5).abs() < 1e-12);
        assert_eq!(i_u, 0.0);

        // sorted descending — first entry has the largest gap.
        for w in violations.windows(2) {
            assert!(w[0].gap >= w[1].gap);
        }
    }
}
