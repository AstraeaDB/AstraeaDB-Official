use serde::Serialize;

use astraea_server::protocol::Request;

use crate::client::ProxyClient;
use crate::errors::McpError;

/// A static MCP resource.
#[derive(Debug, Clone, Serialize)]
pub struct ResourceDefinition {
    pub uri: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

/// An MCP resource template (dynamic URI).
#[derive(Debug, Clone, Serialize)]
pub struct ResourceTemplate {
    #[serde(rename = "uriTemplate")]
    pub uri_template: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

/// Content returned from a resource read.
#[derive(Debug, Clone, Serialize)]
pub struct ResourceContent {
    pub uri: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub text: String,
}

/// Return the list of static resources.
pub fn static_resources() -> Vec<ResourceDefinition> {
    vec![ResourceDefinition {
        uri: "astraea://stats".to_string(),
        name: "Graph Statistics".to_string(),
        description: "Overview of the graph: node count, edge count, and label distribution."
            .to_string(),
        mime_type: "application/json".to_string(),
    }]
}

/// Return the list of resource templates.
pub fn resource_templates() -> Vec<ResourceTemplate> {
    vec![
        ResourceTemplate {
            uri_template: "astraea://node/{id}".to_string(),
            name: "Node".to_string(),
            description: "Full data for a node including labels, properties, and embedding metadata."
                .to_string(),
            mime_type: "application/json".to_string(),
        },
        ResourceTemplate {
            uri_template: "astraea://edge/{id}".to_string(),
            name: "Edge".to_string(),
            description:
                "Full data for an edge including type, properties, weight, and temporal validity."
                    .to_string(),
            mime_type: "application/json".to_string(),
        },
        ResourceTemplate {
            uri_template: "astraea://subgraph/{nodeId}".to_string(),
            name: "Subgraph".to_string(),
            description: "Linearized text representation of the subgraph around a node."
                .to_string(),
            mime_type: "text/plain".to_string(),
        },
        ResourceTemplate {
            uri_template: "astraea://label/{label}".to_string(),
            name: "Nodes by Label".to_string(),
            description: "All node IDs matching a given label.".to_string(),
            mime_type: "application/json".to_string(),
        },
    ]
}

/// Read a resource by URI.
pub async fn read_resource(client: &ProxyClient, uri: &str) -> Result<ResourceContent, McpError> {
    let parsed = parse_uri(uri)?;

    match parsed {
        ParsedUri::Stats => {
            let data = client.send_and_unwrap(&Request::GraphStats).await?;
            Ok(ResourceContent {
                uri: uri.to_string(),
                mime_type: "application/json".to_string(),
                text: serde_json::to_string_pretty(&data)
                    .unwrap_or_else(|_| data.to_string()),
            })
        }
        ParsedUri::Node(id) => {
            let data = client.send_and_unwrap(&Request::GetNode { id }).await?;
            Ok(ResourceContent {
                uri: uri.to_string(),
                mime_type: "application/json".to_string(),
                text: serde_json::to_string_pretty(&data)
                    .unwrap_or_else(|_| data.to_string()),
            })
        }
        ParsedUri::Edge(id) => {
            let data = client.send_and_unwrap(&Request::GetEdge { id }).await?;
            Ok(ResourceContent {
                uri: uri.to_string(),
                mime_type: "application/json".to_string(),
                text: serde_json::to_string_pretty(&data)
                    .unwrap_or_else(|_| data.to_string()),
            })
        }
        ParsedUri::Subgraph { node_id, hops, max_nodes } => {
            let data = client
                .send_and_unwrap(&Request::ExtractSubgraph {
                    center: node_id,
                    hops,
                    max_nodes,
                    format: "structured".to_string(),
                })
                .await?;
            let text = data.as_str().unwrap_or("").to_string();
            Ok(ResourceContent {
                uri: uri.to_string(),
                mime_type: "text/plain".to_string(),
                text,
            })
        }
        ParsedUri::Label(label) => {
            let data = client
                .send_and_unwrap(&Request::FindByLabel { label })
                .await?;
            Ok(ResourceContent {
                uri: uri.to_string(),
                mime_type: "application/json".to_string(),
                text: serde_json::to_string_pretty(&data)
                    .unwrap_or_else(|_| data.to_string()),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// URI parsing
// ---------------------------------------------------------------------------

enum ParsedUri {
    Stats,
    Node(u64),
    Edge(u64),
    Subgraph {
        node_id: u64,
        hops: usize,
        max_nodes: usize,
    },
    Label(String),
}

fn parse_uri(uri: &str) -> Result<ParsedUri, McpError> {
    let path = uri
        .strip_prefix("astraea://")
        .ok_or_else(|| McpError::ResourceNotFound(format!("invalid scheme: {uri}")))?;

    let (path, query) = match path.find('?') {
        Some(idx) => (&path[..idx], Some(&path[idx + 1..])),
        None => (path, None),
    };

    let segments: Vec<&str> = path.split('/').collect();

    match segments.as_slice() {
        ["stats"] => Ok(ParsedUri::Stats),
        ["node", id_str] => {
            let id: u64 = id_str
                .parse()
                .map_err(|_| McpError::InvalidParams(format!("invalid node id: {id_str}")))?;
            Ok(ParsedUri::Node(id))
        }
        ["edge", id_str] => {
            let id: u64 = id_str
                .parse()
                .map_err(|_| McpError::InvalidParams(format!("invalid edge id: {id_str}")))?;
            Ok(ParsedUri::Edge(id))
        }
        ["subgraph", id_str] => {
            let node_id: u64 = id_str
                .parse()
                .map_err(|_| McpError::InvalidParams(format!("invalid node id: {id_str}")))?;

            let mut hops: usize = 2;
            let mut max_nodes: usize = 50;

            if let Some(qs) = query {
                for pair in qs.split('&') {
                    if let Some((key, value)) = pair.split_once('=') {
                        match key {
                            "hops" => {
                                hops = value.parse().unwrap_or(2);
                            }
                            "max" => {
                                max_nodes = value.parse().unwrap_or(50);
                            }
                            _ => {}
                        }
                    }
                }
            }

            Ok(ParsedUri::Subgraph {
                node_id,
                hops,
                max_nodes,
            })
        }
        ["label", label] => Ok(ParsedUri::Label((*label).to_string())),
        _ => Err(McpError::ResourceNotFound(format!(
            "unknown resource: {uri}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stats_uri() {
        let parsed = parse_uri("astraea://stats").unwrap();
        assert!(matches!(parsed, ParsedUri::Stats));
    }

    #[test]
    fn parse_node_uri() {
        let parsed = parse_uri("astraea://node/42").unwrap();
        assert!(matches!(parsed, ParsedUri::Node(42)));
    }

    #[test]
    fn parse_edge_uri() {
        let parsed = parse_uri("astraea://edge/7").unwrap();
        assert!(matches!(parsed, ParsedUri::Edge(7)));
    }

    #[test]
    fn parse_subgraph_uri_with_query() {
        let parsed = parse_uri("astraea://subgraph/10?hops=3&max=100").unwrap();
        match parsed {
            ParsedUri::Subgraph {
                node_id,
                hops,
                max_nodes,
            } => {
                assert_eq!(node_id, 10);
                assert_eq!(hops, 3);
                assert_eq!(max_nodes, 100);
            }
            _ => panic!("expected Subgraph"),
        }
    }

    #[test]
    fn parse_subgraph_uri_defaults() {
        let parsed = parse_uri("astraea://subgraph/5").unwrap();
        match parsed {
            ParsedUri::Subgraph {
                node_id,
                hops,
                max_nodes,
            } => {
                assert_eq!(node_id, 5);
                assert_eq!(hops, 2);
                assert_eq!(max_nodes, 50);
            }
            _ => panic!("expected Subgraph"),
        }
    }

    #[test]
    fn parse_label_uri() {
        let parsed = parse_uri("astraea://label/Person").unwrap();
        assert!(matches!(parsed, ParsedUri::Label(ref l) if l == "Person"));
    }

    #[test]
    fn parse_invalid_scheme() {
        assert!(parse_uri("http://stats").is_err());
    }

    #[test]
    fn parse_unknown_path() {
        assert!(parse_uri("astraea://unknown/path").is_err());
    }
}
