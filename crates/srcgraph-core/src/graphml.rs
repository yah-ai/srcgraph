use std::collections::HashMap;

use petgraph::graph::NodeIndex;
use quick_xml::events::Event;
use quick_xml::Reader;
use thiserror::Error;

use crate::owned::{OwnedClassNode, OwnedGraph};
use crate::EdgeType;

#[derive(Debug, Error)]
pub enum Error {
    #[error("XML: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("JSON for key '{key}': {source}")]
    Json {
        key: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("missing required node attribute '{0}'")]
    MissingAttr(String),
    #[error("invalid integer for '{key}': {source}")]
    ParseInt {
        key: String,
        #[source]
        source: std::num::ParseIntError,
    },
    #[error("unknown edge type '{0}'")]
    UnknownEdgeType(String),
    #[error("UTF-8: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

// ── reader ────────────────────────────────────────────────────────────────────

fn attr_str(e: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<String> {
    e.attributes()
        .filter_map(|a| a.ok())
        .find(|a| a.key.local_name().as_ref() == name)
        .and_then(|a| String::from_utf8(a.value.into_owned()).ok())
}

fn parse_edge_type(s: &str) -> Result<EdgeType, Error> {
    match s {
        "MethodCall" => Ok(EdgeType::MethodCall),
        "FieldRef" => Ok(EdgeType::FieldRef),
        "Inheritance" => Ok(EdgeType::Inheritance),
        other => Err(Error::UnknownEdgeType(other.to_owned())),
    }
}

/// Read a GraphML file produced by the visiting-tool extractor into an [`OwnedGraph`].
pub fn read_graphml(xml: &str) -> Result<OwnedGraph, Error> {
    let mut reader = Reader::from_str(xml);

    let mut graph = OwnedGraph::new();
    let mut node_index: HashMap<String, NodeIndex> = HashMap::new();

    // Flat parsing state — no recursion needed for this schema.
    let mut in_node = false;
    let mut in_edge = false;
    let mut in_data = false;
    let mut current_node_id = String::new();
    let mut current_edge_source = String::new();
    let mut current_edge_target = String::new();
    let mut current_data: HashMap<String, String> = HashMap::new();
    let mut current_data_key = String::new();
    // Deferred edges so the graph is correct even if edges precede nodes.
    let mut pending_edges: Vec<(String, String, String)> = Vec::new();

    loop {
        match reader.read_event()? {
            Event::Start(ref e) => match e.name().local_name().as_ref() {
                b"node" => {
                    in_node = true;
                    current_node_id = attr_str(e, b"id").unwrap_or_default();
                    current_data.clear();
                }
                b"edge" => {
                    in_edge = true;
                    current_edge_source = attr_str(e, b"source").unwrap_or_default();
                    current_edge_target = attr_str(e, b"target").unwrap_or_default();
                    current_data.clear();
                }
                b"data" => {
                    in_data = true;
                    current_data_key = attr_str(e, b"key").unwrap_or_default();
                }
                _ => {}
            },
            Event::End(ref e) => match e.name().local_name().as_ref() {
                b"node" => {
                    let node = OwnedClassNode::from_data(&current_node_id, &current_data)?;
                    let idx = graph.add_node(node);
                    node_index.insert(current_node_id.clone(), idx);
                    in_node = false;
                }
                b"edge" => {
                    let type_str = current_data
                        .get("type")
                        .cloned()
                        .unwrap_or_else(|| "MethodCall".to_owned());
                    pending_edges.push((
                        current_edge_source.clone(),
                        current_edge_target.clone(),
                        type_str,
                    ));
                    in_edge = false;
                }
                b"data" => {
                    in_data = false;
                }
                _ => {}
            },
            Event::Text(ref e) => {
                if in_data && (in_node || in_edge) {
                    if let Ok(text) = e.unescape() {
                        current_data.insert(current_data_key.clone(), text.into_owned());
                    }
                }
            }
            // CDATA sections (used by write_graphml for JSON blobs)
            Event::CData(ref e) => {
                if in_data && (in_node || in_edge) {
                    if let Ok(s) = std::str::from_utf8(e.as_ref()) {
                        current_data.insert(current_data_key.clone(), s.to_owned());
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    for (src, tgt, type_str) in pending_edges {
        let edge_type = parse_edge_type(&type_str)?;
        if let (Some(&s), Some(&t)) = (node_index.get(&src), node_index.get(&tgt)) {
            graph.add_edge(s, t, edge_type);
        }
    }

    Ok(graph)
}

// ── writer ────────────────────────────────────────────────────────────────────

/// Serialize an [`OwnedGraph`] to GraphML, matching the visiting-tool extractor's schema.
pub fn write_graphml(graph: &OwnedGraph) -> Result<String, Error> {
    use quick_xml::events::{BytesCData, BytesDecl, BytesEnd, BytesStart, BytesText, Event};
    use quick_xml::Writer;

    let mut w = Writer::new(Vec::new());

    w.write_event(Event::Decl(BytesDecl::new("1.0", Some("utf-8"), None)))?;
    w.write_event(Event::Text(BytesText::new("\n")))?;

    let mut graphml = BytesStart::new("graphml");
    graphml.push_attribute(("xmlns", "http://graphml.graphdrawing.org/xmlns"));
    w.write_event(Event::Start(graphml))?;
    w.write_event(Event::Text(BytesText::new("\n")))?;

    // Key declarations (node attributes)
    let node_keys: &[(&str, &str)] = &[
        ("name", "string"),
        ("namespace", "string"),
        ("lineCount", "int"),
        ("methodCount", "int"),
        ("methodConnectivity", "string"),
        ("halstead_eta1", "int"),
        ("halstead_eta2", "int"),
        ("halstead_N1", "int"),
        ("halstead_N2", "int"),
        ("stateTransitions", "string"),
        ("invariants", "string"),
        ("tenantBranches", "string"),
        ("errorMessages", "string"),
        ("deadCode", "string"),
        ("methodFingerprints", "string"),
        ("magicNumbers", "string"),
        ("callSequences", "string"),
        ("cyclomaticComplexity", "string"),
        ("methodTokens", "string"),
        ("pathConditions", "string"),
    ];
    for (id, attr_type) in node_keys {
        let mut k = BytesStart::new("key");
        k.push_attribute(("id", *id));
        k.push_attribute(("for", "node"));
        k.push_attribute(("attr.name", *id));
        k.push_attribute(("attr.type", *attr_type));
        w.write_event(Event::Empty(k))?;
        w.write_event(Event::Text(BytesText::new("\n")))?;
    }
    {
        let mut k = BytesStart::new("key");
        k.push_attribute(("id", "type"));
        k.push_attribute(("for", "edge"));
        k.push_attribute(("attr.name", "type"));
        k.push_attribute(("attr.type", "string"));
        w.write_event(Event::Empty(k))?;
        w.write_event(Event::Text(BytesText::new("\n")))?;
    }

    let mut graph_elem = BytesStart::new("graph");
    graph_elem.push_attribute(("id", "G"));
    graph_elem.push_attribute(("edgedefault", "directed"));
    w.write_event(Event::Start(graph_elem))?;
    w.write_event(Event::Text(BytesText::new("\n")))?;

    // Nodes
    for idx in graph.node_indices() {
        let n = &graph[idx];
        let mut node_elem = BytesStart::new("node");
        node_elem.push_attribute(("id", n.id.as_str()));
        w.write_event(Event::Start(node_elem))?;
        w.write_event(Event::Text(BytesText::new("\n")))?;

        // Helper: write <data key="k">plain_text</data>
        // Plain values (names, integers) cannot contain XML-special chars.
        macro_rules! text_data {
            ($key:expr, $val:expr) => {{
                let mut d = BytesStart::new("data");
                d.push_attribute(("key", $key));
                w.write_event(Event::Start(d))?;
                w.write_event(Event::Text(BytesText::new($val)))?;
                w.write_event(Event::End(BytesEnd::new("data")))?;
                w.write_event(Event::Text(BytesText::new("\n")))?;
            }};
        }

        // Helper: write <data key="k"><![CDATA[json]]></data>
        // JSON blobs may contain <, >, &, " — CDATA avoids escaping.
        macro_rules! json_data {
            ($key:expr, $val:expr) => {{
                let json = serde_json::to_string($val).unwrap_or_default();
                let mut d = BytesStart::new("data");
                d.push_attribute(("key", $key));
                w.write_event(Event::Start(d))?;
                w.write_event(Event::CData(BytesCData::new(json.as_str())))?;
                w.write_event(Event::End(BytesEnd::new("data")))?;
                w.write_event(Event::Text(BytesText::new("\n")))?;
            }};
        }

        text_data!("name", n.name.as_str());
        text_data!("namespace", n.namespace.as_str());
        text_data!("lineCount", &n.line_count.to_string());
        text_data!("methodCount", &n.method_count.to_string());

        if n.halstead_eta1 > 0 || n.halstead_eta2 > 0 || n.halstead_n1 > 0 || n.halstead_n2 > 0 {
            text_data!("halstead_eta1", &n.halstead_eta1.to_string());
            text_data!("halstead_eta2", &n.halstead_eta2.to_string());
            text_data!("halstead_N1", &n.halstead_n1.to_string());
            text_data!("halstead_N2", &n.halstead_n2.to_string());
        }

        if let Some(mc) = &n.method_connectivity {
            json_data!("methodConnectivity", mc);
        }
        if let Some(v) = &n.method_fingerprints {
            json_data!("methodFingerprints", v);
        }
        if let Some(v) = &n.call_sequences {
            json_data!("callSequences", v);
        }
        if let Some(v) = &n.cyclomatic_complexity {
            json_data!("cyclomaticComplexity", v);
        }
        if let Some(v) = &n.method_tokens {
            json_data!("methodTokens", v);
        }
        if let Some(v) = &n.path_conditions {
            json_data!("pathConditions", v);
        }
        if let Some(v) = &n.invariants {
            json_data!("invariants", v);
        }
        if let Some(v) = &n.error_messages {
            json_data!("errorMessages", v);
        }
        if let Some(v) = &n.magic_numbers {
            json_data!("magicNumbers", v);
        }
        if let Some(v) = &n.dead_code {
            json_data!("deadCode", v);
        }
        if let Some(v) = &n.tenant_branches {
            json_data!("tenantBranches", v);
        }
        if let Some(v) = &n.state_transitions {
            json_data!("stateTransitions", v);
        }

        w.write_event(Event::End(BytesEnd::new("node")))?;
        w.write_event(Event::Text(BytesText::new("\n")))?;
    }

    // Edges
    for edge_idx in graph.edge_indices() {
        let (src_idx, tgt_idx) = graph.edge_endpoints(edge_idx).unwrap();
        let src_id = &graph[src_idx].id;
        let tgt_id = &graph[tgt_idx].id;
        let edge_type = graph.edge_weight(edge_idx).unwrap();

        let mut e = BytesStart::new("edge");
        e.push_attribute(("source", src_id.as_str()));
        e.push_attribute(("target", tgt_id.as_str()));
        w.write_event(Event::Start(e))?;
        w.write_event(Event::Text(BytesText::new("\n")))?;

        let type_str = match edge_type {
            EdgeType::MethodCall => "MethodCall",
            EdgeType::FieldRef => "FieldRef",
            EdgeType::Inheritance => "Inheritance",
        };
        let mut d = BytesStart::new("data");
        d.push_attribute(("key", "type"));
        w.write_event(Event::Start(d))?;
        w.write_event(Event::Text(BytesText::new(type_str)))?;
        w.write_event(Event::End(BytesEnd::new("data")))?;
        w.write_event(Event::Text(BytesText::new("\n")))?;

        w.write_event(Event::End(BytesEnd::new("edge")))?;
        w.write_event(Event::Text(BytesText::new("\n")))?;
    }

    w.write_event(Event::End(BytesEnd::new("graph")))?;
    w.write_event(Event::Text(BytesText::new("\n")))?;
    w.write_event(Event::End(BytesEnd::new("graphml")))?;

    Ok(String::from_utf8(w.into_inner())?)
}
