# srcgraph-metrics

Graph-theoretic code-analysis metrics over the [srcgraph-core](https://crates.io/crates/srcgraph-core)
traits — SCC (circular dependencies), LCOM4 (cohesion), betweenness centrality
(architectural chokepoints), instability + SDP violations, and clone detection.
Each metric is behind its own cargo feature (`default = ["all"]`).

Algorithms are generic over `petgraph::Graph<N, E>` where `N: ClassNode, E: EdgeKind`, so
you can run them against an `OwnedGraph` loaded from GraphML or against a live in-process
graph (e.g. yah's `kg-store::Store::class_graph()`).

```rust
use srcgraph_metrics::{betweenness::compute_betweenness, scc::compute_scc};

let graph = srcgraph_core::OwnedGraph::from_graphml_file("graph.graphml")?;
let scc   = compute_scc(&graph);
let bet   = compute_betweenness(&graph);
```

Benchmarks against networkx on a 7000+ node graph land in the 50× range (`cargo bench
-p srcgraph-metrics --bench betweenness`).

License: MIT OR Apache-2.0.
