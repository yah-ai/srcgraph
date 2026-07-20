use petgraph::Graph;
use std::collections::HashMap;

use crate::{ClassNode, EdgeType, MethodConnectivity};

/// Concrete directed graph loaded from a GraphML file.
pub type OwnedGraph = Graph<OwnedClassNode, EdgeType>;

/// A class node with all attributes from the GraphML schema.
///
/// Required fields (`id`, `name`, `namespace`, `lineCount`, `methodCount`) are always
/// present. Halstead and semantic-mode fields are optional — they are `None` when the
/// GraphML was exported in syntax-only mode.
#[derive(Debug, Clone)]
pub struct OwnedClassNode {
    /// Fully-qualified class name used as the GraphML node id.
    pub id: String,
    pub name: String,
    pub namespace: String,
    pub line_count: u32,
    pub method_count: u32,
    pub halstead_eta1: u32,
    pub halstead_eta2: u32,
    pub halstead_n1: u32,
    pub halstead_n2: u32,
    pub method_connectivity: Option<MethodConnectivity>,
    // Semantic-mode JSON blobs — stored as raw serde_json::Value for downstream metrics.
    pub method_fingerprints: Option<serde_json::Value>,
    pub method_tokens: Option<serde_json::Value>,
    pub call_sequences: Option<serde_json::Value>,
    pub cyclomatic_complexity: Option<serde_json::Value>,
    pub path_conditions: Option<serde_json::Value>,
    pub invariants: Option<serde_json::Value>,
    pub error_messages: Option<serde_json::Value>,
    pub magic_numbers: Option<serde_json::Value>,
    pub dead_code: Option<serde_json::Value>,
    pub tenant_branches: Option<serde_json::Value>,
    pub state_transitions: Option<serde_json::Value>,
}

impl OwnedClassNode {
    pub(crate) fn from_data(
        id: &str,
        data: &HashMap<String, String>,
    ) -> Result<Self, crate::graphml::Error> {
        use crate::graphml::Error;

        let require = |key: &str| -> Result<&str, Error> {
            data.get(key)
                .map(String::as_str)
                .ok_or_else(|| Error::MissingAttr(key.to_owned()))
        };
        let require_u32 = |key: &str| -> Result<u32, Error> {
            require(key)?
                .parse::<u32>()
                .map_err(|e| Error::ParseInt { key: key.to_owned(), source: e })
        };
        let opt_u32 = |key: &str| -> u32 {
            data.get(key).and_then(|s| s.parse().ok()).unwrap_or(0)
        };
        let opt_json = |key: &str| -> Result<Option<serde_json::Value>, Error> {
            data.get(key)
                .map(|s| {
                    serde_json::from_str(s)
                        .map_err(|e| Error::Json { key: key.to_owned(), source: e })
                })
                .transpose()
        };

        Ok(OwnedClassNode {
            id: id.to_owned(),
            name: require("name")?.to_owned(),
            namespace: require("namespace")?.to_owned(),
            line_count: require_u32("lineCount")?,
            method_count: require_u32("methodCount")?,
            halstead_eta1: opt_u32("halstead_eta1"),
            halstead_eta2: opt_u32("halstead_eta2"),
            halstead_n1: opt_u32("halstead_N1"),
            halstead_n2: opt_u32("halstead_N2"),
            method_connectivity: data
                .get("methodConnectivity")
                .map(|s| {
                    serde_json::from_str(s).map_err(|e| Error::Json {
                        key: "methodConnectivity".to_owned(),
                        source: e,
                    })
                })
                .transpose()?,
            method_fingerprints: opt_json("methodFingerprints")?,
            method_tokens: opt_json("methodTokens")?,
            call_sequences: opt_json("callSequences")?,
            cyclomatic_complexity: opt_json("cyclomaticComplexity")?,
            path_conditions: opt_json("pathConditions")?,
            invariants: opt_json("invariants")?,
            error_messages: opt_json("errorMessages")?,
            magic_numbers: opt_json("magicNumbers")?,
            dead_code: opt_json("deadCode")?,
            tenant_branches: opt_json("tenantBranches")?,
            state_transitions: opt_json("stateTransitions")?,
        })
    }
}

impl ClassNode for OwnedClassNode {
    fn id(&self) -> &str {
        &self.id
    }
    fn namespace(&self) -> &str {
        &self.namespace
    }
    fn line_count(&self) -> u32 {
        self.line_count
    }
    fn method_count(&self) -> u32 {
        self.method_count
    }
    fn method_connectivity(&self) -> Option<&MethodConnectivity> {
        self.method_connectivity.as_ref()
    }
    fn cyclomatic_complexity(&self) -> Option<&serde_json::Value> {
        self.cyclomatic_complexity.as_ref()
    }
    fn halstead_eta1(&self) -> u32 {
        self.halstead_eta1
    }
    fn halstead_eta2(&self) -> u32 {
        self.halstead_eta2
    }
    fn halstead_n1(&self) -> u32 {
        self.halstead_n1
    }
    fn halstead_n2(&self) -> u32 {
        self.halstead_n2
    }
    fn method_tokens(&self) -> Option<&serde_json::Value> {
        self.method_tokens.as_ref()
    }
    fn method_fingerprints(&self) -> Option<&serde_json::Value> {
        self.method_fingerprints.as_ref()
    }
    fn call_sequences(&self) -> Option<&serde_json::Value> {
        self.call_sequences.as_ref()
    }
}

