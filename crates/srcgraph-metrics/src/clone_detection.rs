//! Clone detection via n-gram Jaccard similarity on per-method fingerprint
//! token streams.
//!
//! Each class node carries a `methodFingerprints` JSON blob with shape
//!
//!   `{"methods": [{"name": str, "tokens": "KW_IF BINOP INVOKE …", "line": int, …}, …]}`
//!
//! where `tokens` is a space-separated string of normalised token classes
//! emitted by the extractor. We:
//!
//! 1. Build the n-gram set (default `n = 3`) of each method's token stream.
//! 2. Score every cross-class method pair by Jaccard similarity
//!    `|A ∩ B| / |A ∪ B|`. Pairs with `sim ≥ threshold` (default 0.7) are
//!    emitted as clone pairs.
//! 3. Run union-find over the clone-pair graph to collapse pairs into
//!    transitive clone families.
//!
//! Two practical filters mirror the Python reference (`analysis/clone_detection.py`):
//!
//! - **token-count ratio gate** — skip pairs whose token counts differ by
//!   more than 2× before computing n-grams (cheap O(1) prune).
//! - **same-class same-name skip** — partial-class siblings would otherwise
//!   self-match.
//!
//! See `DESIGN.md` (Phase 3) at the workspace root.

use petgraph::Graph;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use srcgraph_core::{ClassNode, EdgeKind};

/// One method's clone-detection record, harvested from a class's
/// `methodFingerprints` blob and tagged with the owning class.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodFingerprint {
    pub node_id: String,
    pub class_name: String,
    pub name: String,
    /// Space-separated token-class string from the extractor.
    pub tokens: String,
    /// Source line (best-effort; 0 when absent).
    pub line: u32,
    pub end_line: u32,
    pub params: u32,
}

/// A similarity edge between two methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClonePair {
    pub m1: MethodFingerprint,
    pub m2: MethodFingerprint,
    pub similarity: f64,
}

/// A connected component over the clone-pair graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneFamily {
    pub id: usize,
    pub size: usize,
    pub members: Vec<MethodFingerprint>,
    /// Mean similarity across in-family pairs.
    pub avg_similarity: f64,
}

/// Whole-graph clone-detection readout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneAnalysis {
    pub pairs: Vec<ClonePair>,
    pub families: Vec<CloneFamily>,
    pub total_methods: usize,
    /// Distinct (`node_id`, `name`) keys that participate in any clone pair.
    pub cloned_methods: usize,
    pub clone_ratio: f64,
}

/// Parse the `{"methods": [...]}` fingerprint blob attached to a single class.
///
/// Returns `None` if the blob isn't an object with a `methods` array; missing
/// per-field values default (e.g. `line = 0`, `tokens = ""`).
pub fn parse_method_fingerprints(
    blob: &serde_json::Value,
    node_id: &str,
    class_name: &str,
) -> Option<Vec<MethodFingerprint>> {
    let methods = blob.get("methods")?.as_array()?;
    let mut out = Vec::with_capacity(methods.len());
    for m in methods {
        let Some(obj) = m.as_object() else { continue };
        let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("").to_owned();
        let tokens = obj.get("tokens").and_then(|v| v.as_str()).unwrap_or("").to_owned();
        let line = obj.get("line").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let end_line = obj.get("endLine").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let params = obj.get("params").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        out.push(MethodFingerprint {
            node_id: node_id.to_owned(),
            class_name: class_name.to_owned(),
            name,
            tokens,
            line,
            end_line,
            params,
        });
    }
    Some(out)
}

/// Build the set of n-grams of a space-separated token string.
///
/// Mirrors the Python reference: a stream shorter than `n` collapses to the
/// single full-stream tuple (or the empty set when the stream is empty).
/// N-grams are joined back into a `String` with `'\u{1f}'` (unit separator) as
/// a delimiter — chosen because it cannot appear inside a normalised token
/// class — so hashing / equality stay cheap.
pub fn extract_ngrams(token_str: &str, n: usize) -> HashSet<String> {
    let tokens: Vec<&str> = token_str.split_whitespace().collect();
    let mut out = HashSet::new();
    if tokens.is_empty() {
        return out;
    }
    if tokens.len() < n {
        out.insert(tokens.join("\u{1f}"));
        return out;
    }
    for win in tokens.windows(n) {
        out.insert(win.join("\u{1f}"));
    }
    out
}

/// Jaccard similarity on n-gram sets of two token strings. Empty-on-both
/// yields `0.0` (matches Python).
pub fn ngram_jaccard(a_tokens: &str, b_tokens: &str, n: usize) -> f64 {
    let a = extract_ngrams(a_tokens, n);
    let b = extract_ngrams(b_tokens, n);
    jaccard(&a, &b)
}

fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count();
    let union = a.len() + b.len() - inter;
    if union == 0 {
        0.0
    } else {
        inter as f64 / union as f64
    }
}

/// Detect clone pairs among methods by n-gram Jaccard similarity. O(n²) over
/// the method list — caller is responsible for capping graph size (the Python
/// reference skips graphs above 500 nodes).
pub fn detect_clone_pairs(
    methods: &[MethodFingerprint],
    threshold: f64,
    n: usize,
) -> Vec<ClonePair> {
    let ngram_sets: Vec<HashSet<String>> = methods
        .iter()
        .map(|m| extract_ngrams(&m.tokens, n))
        .collect();
    let token_counts: Vec<usize> = methods
        .iter()
        .map(|m| m.tokens.split_whitespace().count())
        .collect();

    let mut pairs = Vec::new();
    for i in 0..methods.len() {
        for j in (i + 1)..methods.len() {
            // Cheap O(1) prune: skip if token-count ratio > 2:1.
            let (ci, cj) = (token_counts[i], token_counts[j]);
            if ci > 0 && cj > 0 {
                let (lo, hi) = if ci < cj { (ci, cj) } else { (cj, ci) };
                if (hi as f64) / (lo as f64) > 2.0 {
                    continue;
                }
            }
            // Skip same-class same-name (partial-class sibling, not a clone).
            if methods[i].node_id == methods[j].node_id
                && methods[i].name == methods[j].name
            {
                continue;
            }
            let a = &ngram_sets[i];
            let b = &ngram_sets[j];
            if a.is_empty() || b.is_empty() {
                continue;
            }
            let sim = jaccard(a, b);
            if sim >= threshold {
                pairs.push(ClonePair {
                    m1: methods[i].clone(),
                    m2: methods[j].clone(),
                    similarity: (sim * 10_000.0).round() / 10_000.0,
                });
            }
        }
    }
    pairs
}

fn method_key(m: &MethodFingerprint) -> String {
    format!("{}::{}:{}", m.node_id, m.name, m.line)
}

/// Group clone pairs into transitive families via union-find. Returns families
/// sorted by descending member count.
pub fn group_clone_families(pairs: &[ClonePair]) -> Vec<CloneFamily> {
    if pairs.is_empty() {
        return Vec::new();
    }

    // Compact integer ids for union-find.
    let mut key_to_idx: HashMap<String, usize> = HashMap::new();
    let mut methods_by_idx: Vec<MethodFingerprint> = Vec::new();
    let mut pair_idx: Vec<(usize, usize)> = Vec::with_capacity(pairs.len());

    for p in pairs {
        let k1 = method_key(&p.m1);
        let k2 = method_key(&p.m2);
        let i1 = *key_to_idx.entry(k1).or_insert_with(|| {
            methods_by_idx.push(p.m1.clone());
            methods_by_idx.len() - 1
        });
        let i2 = *key_to_idx.entry(k2).or_insert_with(|| {
            methods_by_idx.push(p.m2.clone());
            methods_by_idx.len() - 1
        });
        pair_idx.push((i1, i2));
    }

    let mut parent: Vec<usize> = (0..methods_by_idx.len()).collect();
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    for &(a, b) in &pair_idx {
        let ra = find(&mut parent, a);
        let rb = find(&mut parent, b);
        if ra != rb {
            parent[ra] = rb;
        }
    }

    // Bucket members by root.
    let mut buckets: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..methods_by_idx.len() {
        let r = find(&mut parent, i);
        buckets.entry(r).or_default().push(i);
    }

    // Pair-similarity lookup keyed on sorted index pair.
    let mut pair_sim: HashMap<(usize, usize), f64> = HashMap::new();
    for (idx, p) in pairs.iter().enumerate() {
        let (a, b) = pair_idx[idx];
        let k = if a < b { (a, b) } else { (b, a) };
        pair_sim.insert(k, p.similarity);
    }

    let mut families: Vec<CloneFamily> = buckets
        .into_iter()
        .map(|(_root, members)| {
            // Average similarity across in-family pairs.
            let mut sum = 0.0;
            let mut count = 0usize;
            let mset: HashSet<usize> = members.iter().copied().collect();
            for (&(a, b), &sim) in &pair_sim {
                if mset.contains(&a) && mset.contains(&b) {
                    sum += sim;
                    count += 1;
                }
            }
            let avg = if count > 0 { sum / count as f64 } else { 0.0 };
            CloneFamily {
                id: 0,
                size: members.len(),
                members: members.into_iter().map(|i| methods_by_idx[i].clone()).collect(),
                avg_similarity: (avg * 10_000.0).round() / 10_000.0,
            }
        })
        .collect();

    // Sort by size desc, then assign deterministic family ids.
    families.sort_by(|a, b| b.size.cmp(&a.size));
    for (i, f) in families.iter_mut().enumerate() {
        f.id = i;
    }
    families
}

/// Run clone detection across every node's `methodFingerprints` blob.
///
/// `threshold`/`n` follow the Python reference defaults of 0.7 / 3 when
/// callers pass those; pure-Rust callers can tune them.
pub fn compute_clone_analysis<N, E>(
    graph: &Graph<N, E>,
    threshold: f64,
    n: usize,
) -> CloneAnalysis
where
    N: ClassNode,
    E: EdgeKind,
{
    let mut all_methods: Vec<MethodFingerprint> = Vec::new();
    for nx in graph.node_indices() {
        let node = &graph[nx];
        let Some(blob) = node.method_fingerprints() else {
            continue;
        };
        // The blob can be either the {methods: [...]} object or a JSON string
        // when stored verbatim from GraphML. Handle both.
        let parsed = if let Some(s) = blob.as_str() {
            serde_json::from_str::<serde_json::Value>(s)
                .ok()
                .and_then(|v| parse_method_fingerprints(&v, node.id(), node.id()))
        } else {
            parse_method_fingerprints(blob, node.id(), node.id())
        };
        if let Some(mut ms) = parsed {
            all_methods.append(&mut ms);
        }
    }

    let pairs = detect_clone_pairs(&all_methods, threshold, n);
    let families = group_clone_families(&pairs);

    let mut cloned_keys: HashSet<String> = HashSet::new();
    for p in &pairs {
        cloned_keys.insert(format!("{}::{}", p.m1.node_id, p.m1.name));
        cloned_keys.insert(format!("{}::{}", p.m2.node_id, p.m2.name));
    }
    let total = all_methods.len();
    let clone_ratio = if total > 0 {
        (cloned_keys.len() as f64 / total as f64 * 10_000.0).round() / 10_000.0
    } else {
        0.0
    };

    CloneAnalysis {
        pairs,
        families,
        total_methods: total,
        cloned_methods: cloned_keys.len(),
        clone_ratio,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use srcgraph_core::{OwnedClassNode, OwnedGraph};
    use petgraph::Graph;
    use serde_json::json;

    fn class(id: &str, fingerprints: Option<serde_json::Value>) -> OwnedClassNode {
        OwnedClassNode {
            id: id.to_owned(),
            name: id.to_owned(),
            namespace: "test".to_owned(),
            line_count: 10,
            method_count: 1,
            halstead_eta1: 0,
            halstead_eta2: 0,
            halstead_n1: 0,
            halstead_n2: 0,
            method_connectivity: None,
            method_fingerprints: fingerprints,
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

    fn fp(name: &str, tokens: &str, line: u32) -> serde_json::Value {
        json!({"name": name, "tokens": tokens, "line": line, "endLine": line + 10, "params": 0})
    }

    #[test]
    fn ngrams_short_stream_collapses_to_one_tuple() {
        let ng = extract_ngrams("A B", 3);
        assert_eq!(ng.len(), 1);
        assert!(ng.contains(&"A\u{1f}B".to_owned()));
    }

    #[test]
    fn ngrams_basic_trigrams() {
        let ng = extract_ngrams("A B C D E", 3);
        assert_eq!(ng.len(), 3);
        assert!(ng.contains(&"A\u{1f}B\u{1f}C".to_owned()));
        assert!(ng.contains(&"C\u{1f}D\u{1f}E".to_owned()));
    }

    #[test]
    fn ngrams_empty_yields_empty() {
        assert!(extract_ngrams("", 3).is_empty());
    }

    #[test]
    fn jaccard_identical_is_one() {
        let s = "KW_IF BINOP INVOKE KW_RETURN";
        assert!((ngram_jaccard(s, s, 3) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn jaccard_disjoint_is_zero() {
        let a = "X Y Z W V";
        let b = "P Q R S T";
        assert_eq!(ngram_jaccard(a, b, 3), 0.0);
    }

    #[test]
    fn detect_pairs_identical_methods_match() {
        let toks = "KW_IF BINOP INVOKE KW_RETURN KW_ELSE";
        let methods = vec![
            MethodFingerprint {
                node_id: "A".into(),
                class_name: "A".into(),
                name: "foo".into(),
                tokens: toks.into(),
                line: 1,
                end_line: 10,
                params: 0,
            },
            MethodFingerprint {
                node_id: "B".into(),
                class_name: "B".into(),
                name: "bar".into(),
                tokens: toks.into(),
                line: 1,
                end_line: 10,
                params: 0,
            },
        ];
        let pairs = detect_clone_pairs(&methods, 0.7, 3);
        assert_eq!(pairs.len(), 1);
        assert!((pairs[0].similarity - 1.0).abs() < 1e-12);
    }

    #[test]
    fn detect_pairs_skips_token_count_outliers() {
        // 2 tokens vs 10 tokens — ratio 5:1, pruned regardless of overlap.
        let methods = vec![
            MethodFingerprint {
                node_id: "A".into(),
                class_name: "A".into(),
                name: "small".into(),
                tokens: "KW_IF BINOP".into(),
                line: 1,
                end_line: 2,
                params: 0,
            },
            MethodFingerprint {
                node_id: "B".into(),
                class_name: "B".into(),
                name: "big".into(),
                tokens: "KW_IF BINOP INVOKE KW_RETURN KW_ELSE KW_FOR KW_WHILE KW_DO KW_TRY KW_CATCH".into(),
                line: 1,
                end_line: 20,
                params: 0,
            },
        ];
        let pairs = detect_clone_pairs(&methods, 0.0, 3);
        assert!(pairs.is_empty());
    }

    #[test]
    fn detect_pairs_skips_same_class_same_name() {
        // Same node_id and name: partial-class sibling, not a clone.
        let toks = "KW_IF BINOP INVOKE";
        let methods = vec![
            MethodFingerprint {
                node_id: "A".into(),
                class_name: "A".into(),
                name: "foo".into(),
                tokens: toks.into(),
                line: 1,
                end_line: 5,
                params: 0,
            },
            MethodFingerprint {
                node_id: "A".into(),
                class_name: "A".into(),
                name: "foo".into(),
                tokens: toks.into(),
                line: 100,
                end_line: 105,
                params: 0,
            },
        ];
        assert!(detect_clone_pairs(&methods, 0.5, 3).is_empty());
    }

    #[test]
    fn families_union_transitive_chain() {
        // foo ~ bar, bar ~ baz → one family of size 3.
        let mk = |n: &str, line: u32| MethodFingerprint {
            node_id: n.into(),
            class_name: n.into(),
            name: n.into(),
            tokens: String::new(),
            line,
            end_line: line + 1,
            params: 0,
        };
        let foo = mk("foo", 1);
        let bar = mk("bar", 1);
        let baz = mk("baz", 1);
        let pairs = vec![
            ClonePair {
                m1: foo.clone(),
                m2: bar.clone(),
                similarity: 0.9,
            },
            ClonePair {
                m1: bar.clone(),
                m2: baz.clone(),
                similarity: 0.8,
            },
        ];
        let fams = group_clone_families(&pairs);
        assert_eq!(fams.len(), 1);
        assert_eq!(fams[0].size, 3);
        assert!((fams[0].avg_similarity - 0.85).abs() < 1e-6);
    }

    #[test]
    fn families_sorted_by_size_desc() {
        let mk = |n: &str| MethodFingerprint {
            node_id: n.into(),
            class_name: n.into(),
            name: n.into(),
            tokens: String::new(),
            line: 1,
            end_line: 2,
            params: 0,
        };
        let pairs = vec![
            // Small family of 2.
            ClonePair {
                m1: mk("a"),
                m2: mk("b"),
                similarity: 0.8,
            },
            // Large family of 3.
            ClonePair {
                m1: mk("p"),
                m2: mk("q"),
                similarity: 0.9,
            },
            ClonePair {
                m1: mk("q"),
                m2: mk("r"),
                similarity: 0.9,
            },
        ];
        let fams = group_clone_families(&pairs);
        assert_eq!(fams.len(), 2);
        assert_eq!(fams[0].size, 3);
        assert_eq!(fams[0].id, 0);
        assert_eq!(fams[1].size, 2);
        assert_eq!(fams[1].id, 1);
    }

    #[test]
    fn compute_clone_analysis_walks_graph() {
        let mut g: OwnedGraph = Graph::new();
        let toks = "KW_IF BINOP INVOKE KW_RETURN KW_ELSE";
        g.add_node(class("A", Some(json!({"methods": [fp("foo", toks, 1)]}))));
        g.add_node(class("B", Some(json!({"methods": [fp("bar", toks, 1)]}))));
        // Distinct stream — should not match either.
        g.add_node(class("C", Some(json!({"methods": [fp("baz", "X Y Z W V", 1)]}))));
        // Missing blob — silently skipped.
        g.add_node(class("D", None));

        let r = compute_clone_analysis(&g, 0.7, 3);
        assert_eq!(r.total_methods, 3);
        assert_eq!(r.pairs.len(), 1);
        assert_eq!(r.families.len(), 1);
        assert_eq!(r.families[0].size, 2);
        assert_eq!(r.cloned_methods, 2);
        let expected = (2.0_f64 / 3.0 * 10_000.0).round() / 10_000.0;
        assert!((r.clone_ratio - expected).abs() < 1e-6);
    }

    #[test]
    fn compute_clone_analysis_accepts_string_encoded_blob() {
        // GraphML round-trip can land the blob as a JSON-string-of-JSON-string.
        let toks = "KW_IF BINOP INVOKE KW_RETURN KW_ELSE";
        let inner = json!({"methods": [fp("foo", toks, 1)]});
        let mut g: OwnedGraph = Graph::new();
        g.add_node(class("A", Some(serde_json::Value::String(inner.to_string()))));
        g.add_node(class("B", Some(json!({"methods": [fp("bar", toks, 1)]}))));

        let r = compute_clone_analysis(&g, 0.7, 3);
        assert_eq!(r.total_methods, 2);
        assert_eq!(r.pairs.len(), 1);
    }
}
