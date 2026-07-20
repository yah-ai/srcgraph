# srcgraph-core

Graph model, traits, and GraphML I/O shared by the [srcgraph](https://github.com/yah-ai/srcgraph)
metric crates.

- **Traits**: `ClassNode`, `EdgeKind` — the minimum interface every algorithm needs from a graph.
- **Owned types**: `OwnedGraph` (wraps `petgraph::Graph`), `OwnedClassNode`, `EdgeType`.
- **GraphML reader/writer** for the schema produced by the visiting code-analysis extractor: node attrs include `id`, `name`, `namespace`, `lineCount`, `methodCount`, plus JSON-encoded per-method fields in semantic mode (`methodConnectivity`, `methodFingerprints`, `methodTokens`, `callSequences`, `cyclomaticComplexity`, `pathConditions`, `invariants`, `errorMessages`, `magicNumbers`, `deadCode`, `tenantBranches`, `stateTransitions`).

Designed so consumers — [`srcgraph-metrics`](https://crates.io/crates/srcgraph-metrics)
and external graph stores like yah's `kg-store` — can run the algorithms generically
over any `petgraph::Graph<N, E>` where `N: ClassNode, E: EdgeKind`.

License: MIT OR Apache-2.0.
