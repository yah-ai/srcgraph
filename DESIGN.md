# srcgraph

> **Scope pruned 2026-07-20 (R610).** This doc describes the original 10-metric phased plan.
> The published crate now ships only the five metrics that produce actionable signal on
> yah's Rust/TS graph and whose inputs are actually extracted: **scc, lcom4, betweenness,
> instability, clone_detection**. Halstead / cyclomatic / entropy / association_rules /
> process_mining and the `srcgraph-py` PyO3 crate were cut. Sections below about those (and
> the Python panels) are historical.

Shared Rust crate family for graph-theoretic code-analysis metrics. Extracts the algorithm
layer of [visiting/graph-theory-code-analysis-tool](../../visiting/graph-theory-code-analysis-tool/)
into reusable Rust so the same metrics can power the existing Python panels (via PyO3) and
yah's own knowledge graph (via native Rust integration).

This workspace lives under `yah/external/` during early iteration; intended to move to its
own repo once the API stabilises. `yah/.gitignore` excludes `external/`, so this tree carries
independent git history.

## Goals

- Port the **graph-algorithmic** parts of the Python analysis layer to Rust.
- Expose them as:
  1. A pure-Rust library generic over `petgraph::Graph<N, E>` with trait bounds, so [`kg-store`](../../crates/yah/kg-store/) can call metrics in-process on its own graph types.
  2. A PyO3 module the existing Python panels can drop in as a replacement for hot networkx calls.
  3. A CLI that reads/writes GraphML so the crate stands alone against any extractor.

## Non-goals

- Replacing the Python UI. Dash + Plotly + Cytoscape stay. Panels keep their rendering and structure; only the slow algorithmic steps move into Rust.
- Porting the ML-stats analyses. NMF topic decomposition, MDS, agglomerative clustering, k-means — sklearn is already C/BLAS-backed. A Rust port via `linfa`/`smartcore` would tie or lose; not worth the maintenance cost.
- Implementing extractors. Roslyn (C#) stays in the visiting tool; future Rust extractor uses [`rust-analyzer`](https://github.com/rust-lang/rust-analyzer)'s `ra_ap_*` library crates; future TS extractor uses the TypeScript Compiler API via [`ts-morph`](https://github.com/dsherret/ts-morph). All three emit the same GraphML schema. `syn` / `swc` / `tree-sitter` are second-tier syntax-only fallbacks, not primaries.

## Workspace layout

```
external/srcgraph/
  Cargo.toml             workspace manifest
  DESIGN.md              this doc
  LICENSE-MIT
  LICENSE-APACHE
  crates/
    srcgraph-core/     model + traits + GraphML I/O
    srcgraph-metrics/  algorithms
    srcgraph-py/       PyO3 bindings (built via maturin)
```

## Crate breakdown

### `srcgraph-core`

- Traits: `ClassNode`, `EdgeKind` — minimum interface the algorithms need from a graph.
- Concrete types: `OwnedGraph` (wraps `petgraph::Graph`), `OwnedClassNode`, `EdgeType`.
- GraphML reader/writer matching the schema documented at [visiting/graph-theory-code-analysis-tool/README.md](../../visiting/graph-theory-code-analysis-tool/README.md) (node attrs include `id`, `name`, `namespace`, `lineCount`, `methodCount`, plus JSON-encoded per-method fields in semantic mode).
- Per-method metadata accessors: `methodConnectivity`, `methodFingerprints`, `methodTokens`, `callSequences`, `cyclomaticComplexity`, `pathConditions`, `invariants`, `errorMessages`, `magicNumbers`, `deadCode`, `tenantBranches`, `stateTransitions`.

### `srcgraph-metrics`

Phased so the biggest wins land first.

**Phase 1 — graph algorithms with the largest Python overhead:**
- `scc` — Tarjan via `petgraph::algo::tarjan_scc`; condensation DAG; status rating (ready / partially-blocked / fully-entangled).
- `lcom4` — connected components on the method-field bipartite graph per class.
- `betweenness` — Brandes' algorithm. **Headline perf target** — networkx's O(VE) Python loop is the slowest path in the existing tool.

**Phase 2 — other graph-level metrics:**
- `instability` — Martin's I = Ce / (Ca + Ce); SDP violations.
- `cyclomatic` — per-method CC + bimodality detection (dip statistic).
- `halstead` — V / D / E / B from already-extracted η₁/η₂/N₁/N₂.
- `entropy` — Shannon over identifier tokens.

**Phase 3 — token / sequence analyses (large Python wins):**
- `clone_detection` — n-gram fingerprints over method token streams; similarity pairs; family grouping.
- `association_rules` — apriori-style itemset growth for class-co-occurrence rules.
- `process_mining` — Petri net construction from call sequences; conformance scoring.

**Out of scope (stays Python):**
- NMF topic decomposition, MDS, agglomerative clustering, FCA lattice — sklearn / scipy-backed, no perf win to chase.
- Spectral clustering eigendecomposition (scipy.sparse.linalg).
- All rendering (Plotly, Cytoscape, Dash callbacks).

### `srcgraph-py`

- PyO3 module `srcgraph_metrics` exposing each algorithm from Phases 1–3.
- Build artifact: a wheel via [maturin](https://github.com/PyO3/maturin); the visiting tool's `analysis/requirements.txt` pins the wheel.
- Drop-in semantics: a panel currently calling `nx.betweenness_centrality(G)` calls `srcgraph_metrics.betweenness(G_graphml_path_or_serialized_bytes)`. No panel rewrites; only the algorithm call site.

## Generic trait design (sketch)

```rust
// srcgraph-core
pub trait ClassNode {
    fn id(&self) -> &str;
    fn namespace(&self) -> &str;
    fn line_count(&self) -> u32;
    fn method_count(&self) -> u32;
    fn method_connectivity(&self) -> Option<&MethodConnectivity>;
    // … one accessor per GraphML attribute, all Option for semantic-mode fields
}

pub trait EdgeKind: Copy {
    fn is_method_call(&self) -> bool;
    fn is_field_ref(&self) -> bool;
    fn is_inheritance(&self) -> bool;
}
```

Algorithms in `srcgraph-metrics` take `&petgraph::Graph<N, E>` where `N: ClassNode, E: EdgeKind`.
- `kg-store`'s existing node/edge types implement these traits → zero serialization for in-process metrics on the live workspace graph.
- `OwnedGraph` loaded from GraphML also implements these → external GraphML files work too, no special-case code path.

## Consumers

### yah-kg (primary)

- `kg-store::Graph` node/edge types implement `ClassNode` / `EdgeKind` (may need a thin newtype if upstream types collide).
- Surface: `yah arch metrics --kind scc|betweenness|lcom4` reads the live kg graph and prints results.
- Future: an arch graph panel in [packages/yah/ui](../../packages/yah/ui/) consumes the JSON output.

### visiting/graph-theory-code-analysis-tool (PyO3)

Swap order, easiest payoff first:
1. **`betweenness_panel.py`** — replace `nx.betweenness_centrality` with `srcgraph_metrics.betweenness`. The biggest visible speedup.
2. **`clone_detection.py`** — n-gram + similarity loops in pure Python today, ideal Rust target.
3. **`association_rules.py`** — apriori itemset growth.
4. **`scc.py`, `lcom4.py`** — modest wins; do these for consistency and to retire the parallel implementations.

Other panels stay on networkx until they hit a perf cliff.

### rs-hack

Skipped. Hack-board is frozen ([project_yah_ui_replaces_hack_board](../../../../.claude/projects/-Users-user-ss-yah/memory/project_yah_ui_replaces_hack_board.md)); no new integrations.

## Renderer decisions (recorded here so they don't churn)

- **yah-ui arch graph view**: three.js, embedded in the existing TS frontend. Mature layout libs (`3d-force-graph`, `ngraph`), GPU-instanced rendering handles 100k+ nodes, no language seam beyond the JSON the Rust metrics already emit.
- **wasm-egui**: deferred indefinitely. The cost (importing wgpu + egui's renderer, hand-rolling force-directed layout, foreign chrome inside yah-ui) doesn't justify a single fullscreen tab when three.js handles it. Revisit only if three.js hits an unfixable wall — and at that point a **native-egui standalone viewer** is a better answer than wasm.

## Tauri shell for visiting tool (deferred)

The pattern (Tauri host + Python sidecar serving the existing Dash UI + optional native viewer tab) is sound but unproven. Prove it inside [yah/desktop](../../app/yah/desktop/) first — that tree already does Tauri 2 and already carries the macOS launchd PATH-repair shim ([feedback_macos_gui_path](../../../../.claude/projects/-Users-user-ss-yah/memory/feedback_macos_gui_path.md)) needed for any GUI-bundle Python subprocess. Once the plumbing is proven there, optionally replicate for the visiting tool.

## Implementation order

Ticketed in the **srcgraph camp** under relay **R152** (umbrella: "srcgraph crate family v1"):

1. Scaffold `external/srcgraph/` workspace — `Cargo.toml`, LICENSE files, empty crate manifests, lib.rs stubs. *(done)*
2. **R152-T1** — `srcgraph-core`: `ClassNode` / `EdgeKind` traits + `OwnedGraph` + GraphML reader. Round-trip-test against a sample emitted by the existing C# extractor.
3. **R152-T2** — `srcgraph-metrics::scc`. Smallest algorithm; validates the trait shape against a real metric.
4. **R152-T3** — `srcgraph-metrics::lcom4`. Exercises per-method metadata access.
5. **R152-T4** — `srcgraph-metrics::betweenness`. The perf-validation step; benchmark Rust vs networkx on the same graph.
6. **R152-T5** — `srcgraph-py` PyO3 bindings + maturin wheel.
7. **R152-T6** — Replace the visiting tool's betweenness call site with the Rust path; measure speedup.

Ticketed in the **yah root camp** under relay **R153** (umbrella: "yah ↔ srcgraph integration"):

- **R153-B1** *(bug, medium)* — Nested-camp ID allocator leaks across `.yah/` boundary via auto-discovered git worktree. Discovered while scaffolding this camp: the fresh srcgraph camp's first relay got R152 instead of R001 because `yah board worktrees list` auto-enrolls the outer yah git tree as a sibling.
- **R153-T2** — Implement `srcgraph_core::ClassNode` / `EdgeKind` on `kg-store` node/edge types.
- **R153-F3** — `yah arch metrics <scc|betweenness|lcom4>` CLI subcommand wired through the Rust path.
- **R153-F4** — Wire `srcgraph_metrics` output into `ArchGraphCore.tsx` — size/color nodes by betweenness, group by SCC, badge LCOM4 outliers.

Phase 2 (instability, cyclomatic, halstead, entropy) and Phase 3 (clone_detection, association_rules, process_mining) metrics are deferred until R152-T4's benchmark confirms the Rust path is worth investing further in. Open new tickets under R152 (or a successor relay) once that data lands.

## Licensing

Dual MIT / Apache-2.0, matching the yah workspace policy ([feedback_permissive_licenses](../../../../.claude/projects/-Users-user-ss-yah/memory/feedback_permissive_licenses.md)). All dependencies must be MIT / BSD / Apache-2.0 / ISC; no GPL family.
