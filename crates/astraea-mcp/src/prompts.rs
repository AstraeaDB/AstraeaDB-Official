use serde::Serialize;
use serde_json::Value;

use crate::errors::McpError;

/// An MCP prompt definition.
#[derive(Debug, Clone, Serialize)]
pub struct PromptDefinition {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<PromptArgument>,
}

/// A prompt argument.
#[derive(Debug, Clone, Serialize)]
pub struct PromptArgument {
    pub name: String,
    pub description: String,
    pub required: bool,
}

/// A message returned from `prompts/get`.
#[derive(Debug, Clone, Serialize)]
pub struct PromptMessage {
    pub role: String,
    pub content: PromptContent,
}

#[derive(Debug, Clone, Serialize)]
pub struct PromptContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

/// Return all prompt definitions.
pub fn definitions() -> Vec<PromptDefinition> {
    vec![
        PromptDefinition {
            name: "analyze-node".to_string(),
            description: "Analyze a node: describe its properties, connections, and role in the graph.".to_string(),
            arguments: vec![PromptArgument {
                name: "node_id".to_string(),
                description: "The ID of the node to analyze.".to_string(),
                required: true,
            }],
        },
        PromptDefinition {
            name: "explain-path".to_string(),
            description: "Find and explain the shortest path between two nodes.".to_string(),
            arguments: vec![
                PromptArgument {
                    name: "from".to_string(),
                    description: "Source node ID.".to_string(),
                    required: true,
                },
                PromptArgument {
                    name: "to".to_string(),
                    description: "Target node ID.".to_string(),
                    required: true,
                },
            ],
        },
        PromptDefinition {
            name: "explore-community".to_string(),
            description: "Run community detection and describe the community containing a node.".to_string(),
            arguments: vec![PromptArgument {
                name: "node_id".to_string(),
                description: "The ID of a node whose community to explore.".to_string(),
                required: true,
            }],
        },
        PromptDefinition {
            name: "summarize-graph".to_string(),
            description: "Provide a high-level summary of the graph: size, key labels, and structure.".to_string(),
            arguments: vec![],
        },
        PromptDefinition {
            name: "temporal-diff".to_string(),
            description: "Compare the neighborhood of a node at two different points in time.".to_string(),
            arguments: vec![
                PromptArgument {
                    name: "node_id".to_string(),
                    description: "The node to analyze.".to_string(),
                    required: true,
                },
                PromptArgument {
                    name: "t1".to_string(),
                    description: "First timestamp (epoch milliseconds).".to_string(),
                    required: true,
                },
                PromptArgument {
                    name: "t2".to_string(),
                    description: "Second timestamp (epoch milliseconds).".to_string(),
                    required: true,
                },
            ],
        },
        PromptDefinition {
            name: "rag-query".to_string(),
            description: "Answer a question using graph-augmented retrieval (GraphRAG).".to_string(),
            arguments: vec![PromptArgument {
                name: "question".to_string(),
                description: "The natural language question to answer.".to_string(),
                required: true,
            }],
        },
    ]
}

/// Get a prompt by name, filling in the provided arguments.
pub fn get_prompt(name: &str, args: &Value) -> Result<Vec<PromptMessage>, McpError> {
    match name {
        "analyze-node" => {
            let node_id_str = arg_str(args, "node_id")?;

            Ok(vec![PromptMessage {
                role: "user".to_string(),
                content: PromptContent {
                    content_type: "text".to_string(),
                    text: format!(
                        "Analyze node {node_id_str} in the AstraeaDB graph. \
                         Use the get_node tool to retrieve its data, then use the neighbors tool \
                         to explore its connections. Describe:\n\
                         1. What this node represents (based on its labels and properties)\n\
                         2. Its connections and relationships\n\
                         3. Its role and importance in the graph"
                    ),
                },
            }])
        }
        "explain-path" => {
            let from = arg_str(args, "from")?;
            let to = arg_str(args, "to")?;

            Ok(vec![PromptMessage {
                role: "user".to_string(),
                content: PromptContent {
                    content_type: "text".to_string(),
                    text: format!(
                        "Find and explain the shortest path between node {from} and node {to} \
                         in the AstraeaDB graph. Use the shortest_path tool, then get_node on each \
                         node in the path to understand what connects them. Explain each hop \
                         in the path and what the relationship means."
                    ),
                },
            }])
        }
        "explore-community" => {
            let node_id = arg_str(args, "node_id")?;

            Ok(vec![PromptMessage {
                role: "user".to_string(),
                content: PromptContent {
                    content_type: "text".to_string(),
                    text: format!(
                        "Explore the community containing node {node_id}. Use the run_louvain tool \
                         to detect communities, then identify which community this node belongs to. \
                         Use get_node and neighbors to examine key members. Describe the community's \
                         theme, size, and the role of node {node_id} within it."
                    ),
                },
            }])
        }
        "summarize-graph" => Ok(vec![PromptMessage {
            role: "user".to_string(),
            content: PromptContent {
                content_type: "text".to_string(),
                text: "Provide a high-level summary of the AstraeaDB graph. Use the graph_stats \
                       tool to get counts and labels, then explore a few representative nodes \
                       using find_by_label and get_node. Describe the graph's size, what types \
                       of entities it contains, and the kinds of relationships between them."
                    .to_string(),
            },
        }]),
        "temporal-diff" => {
            let node_id = arg_str(args, "node_id")?;
            let t1 = arg_str(args, "t1")?;
            let t2 = arg_str(args, "t2")?;

            Ok(vec![PromptMessage {
                role: "user".to_string(),
                content: PromptContent {
                    content_type: "text".to_string(),
                    text: format!(
                        "Compare the neighborhood of node {node_id} at two points in time. \
                         Use neighbors_at with timestamp {t1} and then with timestamp {t2}. \
                         Describe what connections existed at each time, what changed between \
                         the two snapshots, and what the changes might signify."
                    ),
                },
            }])
        }
        "rag-query" => {
            let question = args
                .get("question")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    McpError::InvalidParams("missing required argument: question".into())
                })?;

            Ok(vec![PromptMessage {
                role: "user".to_string(),
                content: PromptContent {
                    content_type: "text".to_string(),
                    text: format!(
                        "Use the graph_rag tool to answer the following question using \
                         graph-augmented retrieval:\n\n{question}"
                    ),
                },
            }])
        }
        _ => Err(McpError::PromptNotFound(name.to_string())),
    }
}

/// Extract a string argument value (works for both string and numeric JSON values).
fn arg_str(args: &Value, name: &str) -> Result<String, McpError> {
    let val = args
        .get(name)
        .ok_or_else(|| McpError::InvalidParams(format!("missing required argument: {name}")))?;

    if let Some(s) = val.as_str() {
        Ok(s.to_string())
    } else {
        // Numeric or other types: use JSON representation without quotes.
        Ok(val.to_string())
    }
}
