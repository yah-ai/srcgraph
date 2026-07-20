//! Graph-theoretic code-analysis metrics over a `petgraph::Graph<N, E>` where
//! `N: srcgraph_core::ClassNode` and `E: srcgraph_core::EdgeKind`.
//!
//! Each metric is behind its own cargo feature, so a consumer compiles only
//! what it uses (`default = ["all"]`):
//!
//! - `scc` — strongly-connected components (circular dependencies)
//! - `lcom4` — LCOM4 cohesion (low-cohesion types are split candidates)
//! - `betweenness` — betweenness centrality (architectural chokepoints / god nodes)
//! - `instability` — Martin's instability I = Ce/(Ca+Ce) + Stable-Dependencies-Principle violations
//! - `clone_detection` — n-gram fingerprint clone / near-duplicate detection

#[cfg(feature = "scc")]
pub mod scc;
#[cfg(feature = "lcom4")]
pub mod lcom4;
#[cfg(feature = "betweenness")]
pub mod betweenness;
#[cfg(feature = "instability")]
pub mod instability;
#[cfg(feature = "clone_detection")]
pub mod clone_detection;
