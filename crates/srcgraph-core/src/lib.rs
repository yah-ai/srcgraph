//! Graph model, traits, and GraphML I/O.
//!
//! See `DESIGN.md` at the workspace root for the full plan.
//!
//! @yah:relay(R152, "srcgraph crate family v1 — core + Phase 1 metrics + PyO3 bindings")
//! @yah:at(2026-05-12T22:13:13Z)
//! @yah:status(open)
//! @yah:next("T1 OwnedGraph+GraphML reader → T2 SCC → T3 LCOM4 → T4 Betweenness (benchmark vs networkx) → T5 PyO3 bindings → T6 swap visiting tool's betweenness panel")
//! @arch:see(DESIGN.md)
//!
//! @yah:ticket(R152-T1, "srcgraph-core: OwnedGraph + GraphML reader matching the visiting tool's schema")
//! @yah:assignee(agent:claude)
//! @yah:at(2026-05-12T22:14:22Z)
//! @yah:status(review)
//! @yah:parent(R152)
//! @yah:verify("cargo test -p srcgraph-core graphml_roundtrip")
//! @yah:handoff("Implemented OwnedClassNode + OwnedGraph + GraphML reader/writer in srcgraph-core. All attributes from the visiting-tool schema are parsed (name, namespace, lineCount, methodCount, halstead fields, methodConnectivity as typed MethodConnectivity, all semantic-mode JSON blobs as serde_json::Value). Writer uses CDATA for JSON blobs to avoid XML escaping issues. ClassNode trait extended with method_connectivity() defaulting to None.")
//! @yah:verify("cargo test -p srcgraph-core graphml_roundtrip")

pub mod graphml;
pub mod owned;

pub use graphml::{read_graphml, write_graphml, Error as GraphError};
pub use owned::{OwnedClassNode, OwnedGraph};

use serde::{Deserialize, Serialize};

// ── traits ────────────────────────────────────────────────────────────────────

/// Minimum interface the metric algorithms need from a graph node.
///
/// `method_connectivity` has a default impl returning `None` so that external
/// node types (e.g. `kg-store`) can implement this trait without providing
/// connectivity data until they need LCOM4.
pub trait ClassNode {
    fn id(&self) -> &str;
    fn namespace(&self) -> &str;
    fn line_count(&self) -> u32;
    fn method_count(&self) -> u32;
    fn method_connectivity(&self) -> Option<&MethodConnectivity> {
        None
    }
    /// Raw `cyclomaticComplexity` JSON blob — shape
    /// `{"methods": [{"name": str, "complexity": int}, …]}`.
    /// Default `None` for nodes from extractors that omit it.
    fn cyclomatic_complexity(&self) -> Option<&serde_json::Value> {
        None
    }
    /// Halstead raw counts. Default 0 for nodes without the attribute — η = 0
    /// short-circuits V to 0, so a missing-data class produces the same answer
    /// as a class with no operators or operands.
    fn halstead_eta1(&self) -> u32 {
        0
    }
    fn halstead_eta2(&self) -> u32 {
        0
    }
    fn halstead_n1(&self) -> u32 {
        0
    }
    fn halstead_n2(&self) -> u32 {
        0
    }
    /// Raw `methodTokens` JSON blob — shape
    /// `{"methods": [{"name": str, "tokens": [str]}, …]}`.
    fn method_tokens(&self) -> Option<&serde_json::Value> {
        None
    }
    /// Raw `methodFingerprints` JSON blob — shape
    /// `{"methods": [{"name": str, "tokens": "TOK TOK TOK", "line": int, …}, …]}`.
    /// Distinct from `methodTokens`: fingerprint tokens are space-separated
    /// normalised token-class strings used for clone detection.
    fn method_fingerprints(&self) -> Option<&serde_json::Value> {
        None
    }
    /// Raw `callSequences` JSON blob — shape
    /// `{"sequences": [{"method": str, "calls": [str], …}, …]}`. Each sequence
    /// is the ordered list of method names a single method invokes; used as a
    /// transaction for association-rule mining and as a trace log for process
    /// mining.
    fn call_sequences(&self) -> Option<&serde_json::Value> {
        None
    }
}

/// Minimum interface the metric algorithms need from an edge weight.
pub trait EdgeKind: Copy {
    fn is_method_call(&self) -> bool;
    fn is_field_ref(&self) -> bool;
    fn is_inheritance(&self) -> bool;
}

// ── shared types ─────────────────────────────────────────────────────────────

/// Which methods within a class share field accesses.
///
/// Stored as a JSON-encoded string in the `methodConnectivity` GraphML attribute.
/// The `edges` field lists pairs of method names that touch at least one common field.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MethodConnectivity {
    #[serde(default)]
    pub methods: Vec<String>,
    /// Method-name pairs that share field access (used to build the bipartite graph for LCOM4).
    #[serde(default)]
    pub edges: Vec<(String, String)>,
}

/// Edge kinds matching the GraphML schema emitted by the visiting-tool extractor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeType {
    MethodCall,
    FieldRef,
    Inheritance,
}

impl EdgeKind for EdgeType {
    fn is_method_call(&self) -> bool {
        matches!(self, EdgeType::MethodCall)
    }
    fn is_field_ref(&self) -> bool {
        matches!(self, EdgeType::FieldRef)
    }
    fn is_inheritance(&self) -> bool {
        matches!(self, EdgeType::Inheritance)
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    static SMALL_GRAPHML: &str =
        include_str!("../tests/fixtures/small.graphml");

    #[test]
    fn graphml_roundtrip() {
        // ── first pass: read the fixture ──────────────────────────────────
        let g1 = read_graphml(SMALL_GRAPHML).expect("read original graphml");

        assert_eq!(g1.node_count(), 3, "expected 3 nodes");
        assert_eq!(g1.edge_count(), 2, "expected 2 edges");

        let alpha = g1
            .node_weights()
            .find(|n| n.id == "com.example.Alpha")
            .expect("Alpha node");
        assert_eq!(alpha.name, "Alpha");
        assert_eq!(alpha.namespace, "com.example");
        assert_eq!(alpha.line_count, 42);
        assert_eq!(alpha.method_count, 3);

        let mc = alpha.method_connectivity().expect("Alpha.methodConnectivity");
        assert_eq!(mc.methods.len(), 3);
        assert_eq!(mc.edges.len(), 1);
        assert_eq!(mc.edges[0].0, "foo");
        assert_eq!(mc.edges[0].1, "bar");

        // ── second pass: write then re-read ───────────────────────────────
        let written = write_graphml(&g1).expect("write graphml");
        let g2 = read_graphml(&written).expect("read written graphml");

        assert_eq!(g2.node_count(), 3);
        assert_eq!(g2.edge_count(), 2);

        let alpha2 = g2
            .node_weights()
            .find(|n| n.id == "com.example.Alpha")
            .expect("Alpha node (round-trip)");
        assert_eq!(alpha2.name, "Alpha");
        assert_eq!(alpha2.line_count, 42);

        let mc2 = alpha2.method_connectivity().expect("Alpha.methodConnectivity (round-trip)");
        assert_eq!(mc2.methods, mc.methods);
        assert_eq!(mc2.edges[0].0, "foo");

        // Edge types preserved
        let edge_types: Vec<EdgeType> = g2.edge_weights().copied().collect();
        assert!(edge_types.contains(&EdgeType::MethodCall));
        assert!(edge_types.contains(&EdgeType::Inheritance));
    }
}
