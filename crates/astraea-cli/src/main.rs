//! Command-line entry point for AstraeaDB.
//!
//! Single `[[bin]]` crate (binary name `astraeadb`) exposing six clap
//! subcommands: `Serve`, `Import`, `Export`, `Shell`, `Status`, and
//! `Mcp`. `Serve` runs the TCP `AstraeaServer` and
//! `grpc::run_grpc_server` concurrently over a shared
//! `Arc<dyn GraphOps>` plus `Arc<dyn VectorIndex>`; `Mcp` runs
//! `astraea_mcp::McpServer` over `StdioTransport`.
//!
//! `Serve` opens a [`astraea_storage::DiskStorageEngine`] at
//! `cfg.storage.data_dir` via `open()` so the WAL is replayed and
//! node/edge id allocation resumes from the highest id seen. The
//! vector index dimension and distance metric are configurable via the
//! `[vector]` section of the TOML config file (`dimension`, `metric`).
//! Omitting `[vector]` defaults to 128-dim cosine for back-compatibility.

use clap::{Parser, Subcommand};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "astraeadb")]
#[command(about = "AstraeaDB — AI-First Graph Database")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the AstraeaDB server.
    Serve {
        /// Path to configuration file.
        #[arg(short, long, default_value = "config.toml")]
        config: PathBuf,

        /// Bind address (overrides config file).
        #[arg(long)]
        bind: Option<String>,

        /// Port for the TCP server (overrides config file).
        #[arg(short, long)]
        port: Option<u16>,

        /// Port for the gRPC server. Defaults to 50051.
        #[arg(long, default_value = "50051")]
        grpc_port: u16,
    },

    /// Import data from a JSON file into AstraeaDB.
    Import {
        /// Input file path.
        #[arg(short, long)]
        file: PathBuf,

        /// Format of the input file.
        #[arg(short = 'F', long, default_value = "json")]
        format: String,

        /// Data directory.
        #[arg(short, long, default_value = "data")]
        data_dir: PathBuf,

        /// Server address to connect to.
        #[arg(short, long, default_value = "127.0.0.1:7687")]
        address: String,
    },

    /// Export data from AstraeaDB to a JSON file.
    Export {
        /// Output file path.
        #[arg(short, long)]
        file: PathBuf,

        /// Format of the output file.
        #[arg(short = 'F', long, default_value = "json")]
        format: String,

        /// Data directory.
        #[arg(short, long, default_value = "data")]
        data_dir: PathBuf,

        /// Server address to connect to.
        #[arg(short, long, default_value = "127.0.0.1:7687")]
        address: String,

        /// Maximum node ID to attempt exporting (scans IDs 1..max_id).
        #[arg(long, default_value = "1000")]
        max_id: u64,
    },

    /// Open an interactive query shell.
    Shell {
        /// Server address to connect to.
        #[arg(short, long, default_value = "127.0.0.1:7687")]
        address: String,
    },

    /// Show server status.
    Status {
        /// Server address.
        #[arg(short, long, default_value = "127.0.0.1:7687")]
        address: String,
    },

    /// Start an MCP (Model Context Protocol) server for LLM tool integration.
    Mcp {
        /// AstraeaDB server address for proxy mode.
        #[arg(short, long, default_value = "127.0.0.1:7687")]
        address: String,

        /// Auth token for connecting to the AstraeaDB server.
        #[arg(long)]
        auth_token: Option<String>,
    },
}

/// Configuration file structure.
#[derive(Debug, Deserialize)]
struct Config {
    #[serde(default)]
    server: ServerConfig,
    #[serde(default)]
    storage: StorageConfig,
    #[serde(default)]
    vector: VectorConfig,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct ServerConfig {
    bind_address: String,
    port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1".into(),
            port: 7687,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct StorageConfig {
    data_dir: PathBuf,
    buffer_pool_size: usize,
    wal_dir: PathBuf,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("data"),
            buffer_pool_size: 1024,
            wal_dir: PathBuf::from("data/wal"),
        }
    }
}

/// Vector index configuration.
///
/// Omitting the `[vector]` block in the TOML config file is equivalent to:
/// ```toml
/// [vector]
/// dimension = 128
/// metric = "cosine"
/// ```
/// which preserves back-compatibility with pre-existing persisted indexes.
#[derive(Debug, Deserialize)]
#[serde(default)]
struct VectorConfig {
    /// Embedding dimension for the HNSW vector index.
    /// Must match the dimension of all embeddings inserted into this store.
    dimension: usize,
    /// Distance metric: `"cosine"`, `"euclidean"`, or `"dot_product"` / `"dot"`.
    metric: String,
}

impl Default for VectorConfig {
    fn default() -> Self {
        Self {
            dimension: 128,
            metric: "cosine".into(),
        }
    }
}

/// Map a metric name string (case-insensitive) to [`astraea_core::types::DistanceMetric`].
///
/// Accepted values: `"cosine"`, `"euclidean"`, `"dot_product"`, `"dot"`.
/// Returns an error message string on unknown values.
fn parse_metric(s: &str) -> Result<astraea_core::types::DistanceMetric, String> {
    match s.to_ascii_lowercase().as_str() {
        "cosine" => Ok(astraea_core::types::DistanceMetric::Cosine),
        "euclidean" => Ok(astraea_core::types::DistanceMetric::Euclidean),
        "dot_product" | "dot" => Ok(astraea_core::types::DistanceMetric::DotProduct),
        other => Err(format!(
            "Unknown vector metric '{other}'. \
             Valid values are: cosine, euclidean, dot_product (or dot)."
        )),
    }
}

fn load_config(path: &PathBuf) -> Config {
    match std::fs::read_to_string(path) {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(config) => config,
            Err(e) => {
                eprintln!("Warning: failed to parse config file: {e}");
                Config {
                    server: ServerConfig::default(),
                    storage: StorageConfig::default(),
                    vector: VectorConfig::default(),
                }
            }
        },
        Err(_) => Config {
            server: ServerConfig::default(),
            storage: StorageConfig::default(),
            vector: VectorConfig::default(),
        },
    }
}

// ---------------------------------------------------------------------------
// TCP client helper
// ---------------------------------------------------------------------------

/// Send a single JSON request over TCP and return the parsed JSON response.
/// Each request opens a fresh connection (matches server's per-connection model).
async fn send_request(
    address: &str,
    request: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    let mut stream = TcpStream::connect(address).await?;
    let (reader, mut writer) = stream.split();

    let mut msg = serde_json::to_string(request)?;
    msg.push('\n');
    writer.write_all(msg.as_bytes()).await?;

    let mut reader = BufReader::new(reader);
    let mut response_str = String::new();
    reader.read_line(&mut response_str).await?;

    let response: serde_json::Value = serde_json::from_str(response_str.trim())?;
    Ok(response)
}

/// Send a raw JSON string (already serialized) over TCP. Used by the shell
/// to forward user-typed JSON directly.
async fn send_raw_request(
    address: &str,
    json_line: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    let mut stream = TcpStream::connect(address).await?;
    let (reader, mut writer) = stream.split();

    let mut msg = json_line.to_string();
    if !msg.ends_with('\n') {
        msg.push('\n');
    }
    writer.write_all(msg.as_bytes()).await?;

    let mut reader = BufReader::new(reader);
    let mut response = String::new();
    reader.read_line(&mut response).await?;
    Ok(response.trim().to_string())
}

// ---------------------------------------------------------------------------
// Import command
// ---------------------------------------------------------------------------

async fn run_import(file: &PathBuf, address: &str) -> Result<(), Box<dyn std::error::Error>> {
    let contents = std::fs::read_to_string(file)
        .map_err(|e| format!("Failed to read file '{}': {e}", file.display()))?;

    let items: Vec<serde_json::Value> = serde_json::from_str(&contents)
        .map_err(|e| format!("Failed to parse JSON from '{}': {e}", file.display()))?;

    let mut node_count: u64 = 0;
    let mut edge_count: u64 = 0;
    let mut error_count: u64 = 0;

    for (i, item) in items.iter().enumerate() {
        let item_type = item
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();

        let request = match item_type.as_str() {
            "node" => {
                let labels = item
                    .get("labels")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!([]));
                let properties = item
                    .get("properties")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                let embedding = item.get("embedding").cloned();

                let mut req = serde_json::json!({
                    "type": "CreateNode",
                    "labels": labels,
                    "properties": properties,
                });
                if let Some(emb) = embedding
                    && !emb.is_null()
                {
                    req["embedding"] = emb;
                }
                req
            }
            "edge" => {
                let source = item.get("source").and_then(|v| v.as_u64()).ok_or_else(|| {
                    format!("Item {i}: edge missing 'source' (must be a positive integer)")
                })?;
                let target = item.get("target").and_then(|v| v.as_u64()).ok_or_else(|| {
                    format!("Item {i}: edge missing 'target' (must be a positive integer)")
                })?;
                let edge_type = item
                    .get("edge_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("RELATED_TO");
                let properties = item
                    .get("properties")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                let weight = item.get("weight").and_then(|v| v.as_f64()).unwrap_or(1.0);

                let mut req = serde_json::json!({
                    "type": "CreateEdge",
                    "source": source,
                    "target": target,
                    "edge_type": edge_type,
                    "properties": properties,
                    "weight": weight,
                });

                // Pass through optional temporal fields.
                if let Some(vf) = item.get("valid_from") {
                    req["valid_from"] = vf.clone();
                }
                if let Some(vt) = item.get("valid_to") {
                    req["valid_to"] = vt.clone();
                }

                req
            }
            other => {
                eprintln!(
                    "Warning: item {i} has unknown type '{}', skipping.",
                    if other.is_empty() { "<missing>" } else { other }
                );
                error_count += 1;
                continue;
            }
        };

        match send_request(address, &request).await {
            Ok(resp) => {
                let status = resp
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                if status == "ok" {
                    match item_type.as_str() {
                        "node" => node_count += 1,
                        "edge" => edge_count += 1,
                        _ => {}
                    }
                } else {
                    let msg = resp
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown error");
                    eprintln!("Error importing item {i}: {msg}");
                    error_count += 1;
                }
            }
            Err(e) => {
                eprintln!("Connection error on item {i}: {e}");
                error_count += 1;
            }
        }
    }

    println!("Imported {node_count} nodes and {edge_count} edges");
    if error_count > 0 {
        eprintln!("{error_count} items failed to import");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Export command
// ---------------------------------------------------------------------------

async fn run_export(
    file: &PathBuf,
    address: &str,
    max_id: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut exported: Vec<serde_json::Value> = Vec::new();
    let mut node_count: u64 = 0;
    let mut edge_count: u64 = 0;

    println!("Scanning node IDs 1..{max_id}...");

    // Export nodes by scanning IDs.
    for id in 1..=max_id {
        let request = serde_json::json!({
            "type": "GetNode",
            "id": id,
        });

        match send_request(address, &request).await {
            Ok(resp) => {
                let status = resp
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                if status == "ok"
                    && let Some(data) = resp.get("data")
                {
                    let mut node_obj = serde_json::json!({
                        "type": "node",
                        "id": id,
                    });
                    // Copy fields from the response data.
                    if let Some(labels) = data.get("labels") {
                        node_obj["labels"] = labels.clone();
                    }
                    if let Some(props) = data.get("properties") {
                        node_obj["properties"] = props.clone();
                    }
                    if let Some(emb) = data.get("embedding") {
                        node_obj["embedding"] = emb.clone();
                    }
                    exported.push(node_obj);
                    node_count += 1;
                }
                // If status is "error", the node doesn't exist; skip silently.
            }
            Err(e) => {
                eprintln!("Connection error fetching node {id}: {e}");
                // If we get a connection error, the server may be down. Stop.
                break;
            }
        }
    }

    // Export edges by scanning IDs.
    println!("Scanning edge IDs 1..{max_id}...");
    for id in 1..=max_id {
        let request = serde_json::json!({
            "type": "GetEdge",
            "id": id,
        });

        match send_request(address, &request).await {
            Ok(resp) => {
                let status = resp
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                if status == "ok"
                    && let Some(data) = resp.get("data")
                {
                    let mut edge_obj = serde_json::json!({
                        "type": "edge",
                        "id": id,
                    });
                    if let Some(v) = data.get("source") {
                        edge_obj["source"] = v.clone();
                    }
                    if let Some(v) = data.get("target") {
                        edge_obj["target"] = v.clone();
                    }
                    if let Some(v) = data.get("edge_type") {
                        edge_obj["edge_type"] = v.clone();
                    }
                    if let Some(v) = data.get("properties") {
                        edge_obj["properties"] = v.clone();
                    }
                    if let Some(v) = data.get("weight") {
                        edge_obj["weight"] = v.clone();
                    }
                    if let Some(v) = data.get("valid_from") {
                        edge_obj["valid_from"] = v.clone();
                    }
                    if let Some(v) = data.get("valid_to") {
                        edge_obj["valid_to"] = v.clone();
                    }
                    exported.push(edge_obj);
                    edge_count += 1;
                }
            }
            Err(e) => {
                eprintln!("Connection error fetching edge {id}: {e}");
                break;
            }
        }
    }

    // Write to file.
    let json_out = serde_json::to_string_pretty(&exported)?;
    std::fs::write(file, json_out)
        .map_err(|e| format!("Failed to write to '{}': {e}", file.display()))?;

    println!(
        "Exported {node_count} nodes and {edge_count} edges to '{}'",
        file.display()
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Shell command (interactive REPL with rustyline)
// ---------------------------------------------------------------------------

fn run_shell_blocking(address: String) {
    use rustyline::DefaultEditor;
    use rustyline::error::ReadlineError;

    let mut rl = match DefaultEditor::new() {
        Ok(editor) => editor,
        Err(e) => {
            eprintln!("Failed to initialize readline: {e}");
            return;
        }
    };

    // Load history if available.
    let history_path = dirs_history_path();
    if let Some(ref path) = history_path {
        let _ = rl.load_history(path);
    }

    println!("AstraeaDB Interactive Shell");
    println!("Type GQL queries (MATCH, CREATE, DELETE) or special commands.");
    println!("Type .help for available commands. Ctrl-D or .quit to exit.\n");

    // Verify connectivity with a ping.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to build tokio runtime for shell");

    match rt.block_on(send_request(&address, &serde_json::json!({"type": "Ping"}))) {
        Ok(resp) => {
            let version = resp
                .get("data")
                .and_then(|d| d.get("version"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            println!("Connected to AstraeaDB (version: {version})");
        }
        Err(e) => {
            eprintln!("Warning: could not reach server at {address}: {e}");
            eprintln!("Commands will be attempted but may fail.\n");
        }
    }

    loop {
        match rl.readline("astraea> ") {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(trimmed);

                // Handle dot-commands.
                if trimmed.starts_with('.') {
                    match trimmed.to_lowercase().as_str() {
                        ".help" => {
                            print_shell_help();
                            continue;
                        }
                        ".quit" | ".exit" => {
                            println!("Bye.");
                            break;
                        }
                        ".ping" => {
                            let req = serde_json::json!({"type": "Ping"});
                            match rt.block_on(send_request(&address, &req)) {
                                Ok(resp) => println!("{}", format_response(&resp)),
                                Err(e) => eprintln!("\x1b[31mError: {e}\x1b[0m"),
                            }
                            continue;
                        }
                        ".clear" => {
                            // ANSI escape to clear screen and move cursor to top.
                            print!("\x1b[2J\x1b[H");
                            continue;
                        }
                        _ => {
                            eprintln!("Unknown command: {trimmed}. Type .help for help.");
                            continue;
                        }
                    }
                }

                // Determine if input looks like a GQL query or raw JSON.
                let upper = trimmed.to_uppercase();
                let request_json = if upper.starts_with("MATCH")
                    || upper.starts_with("CREATE")
                    || upper.starts_with("DELETE")
                    || upper.starts_with("RETURN")
                    || upper.starts_with("OPTIONAL")
                    || upper.starts_with("WITH")
                    || upper.starts_with("MERGE")
                    || upper.starts_with("SET")
                    || upper.starts_with("REMOVE")
                {
                    // Treat as GQL query.
                    serde_json::to_string(&serde_json::json!({
                        "type": "Query",
                        "gql": trimmed,
                    }))
                    .unwrap()
                } else if trimmed.starts_with('{') {
                    // Treat as raw JSON request.
                    trimmed.to_string()
                } else {
                    // Assume it is a GQL query anyway.
                    serde_json::to_string(&serde_json::json!({
                        "type": "Query",
                        "gql": trimmed,
                    }))
                    .unwrap()
                };

                match rt.block_on(send_raw_request(&address, &request_json)) {
                    Ok(response_str) => {
                        // Try to parse and pretty-print.
                        match serde_json::from_str::<serde_json::Value>(&response_str) {
                            Ok(resp) => println!("{}", format_response(&resp)),
                            Err(_) => println!("{response_str}"),
                        }
                    }
                    Err(e) => {
                        eprintln!("\x1b[31mError: {e}\x1b[0m");
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl-C: just print a new prompt.
                continue;
            }
            Err(ReadlineError::Eof) => {
                // Ctrl-D: exit.
                println!("Bye.");
                break;
            }
            Err(e) => {
                eprintln!("Readline error: {e}");
                break;
            }
        }
    }

    // Save history.
    if let Some(ref path) = history_path {
        let _ = rl.save_history(path);
    }
}

/// Get the path to the shell history file (~/.astraea_history).
fn dirs_history_path() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .map(|home| format!("{home}/.astraea_history"))
}

/// Print help text for the interactive shell.
fn print_shell_help() {
    println!("AstraeaDB Shell Commands:");
    println!();
    println!("  GQL Queries:");
    println!("    MATCH (n:Person) RETURN n      Query nodes by pattern");
    println!("    CREATE (n:Person {{name:'A'}})   Create a node via GQL");
    println!("    DELETE ...                      Delete via GQL");
    println!();
    println!("  Raw JSON:");
    println!("    {{\"type\":\"Ping\"}}                 Send a raw JSON request");
    println!("    {{\"type\":\"GetNode\",\"id\":1}}       Get a node by ID");
    println!();
    println!("  Special Commands:");
    println!("    .help                           Show this help message");
    println!("    .ping                           Ping the server");
    println!("    .clear                          Clear the screen");
    println!("    .quit / .exit                   Exit the shell");
    println!();
}

/// Format a JSON response for display in the shell.
fn format_response(resp: &serde_json::Value) -> String {
    let status = resp
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    match status {
        "ok" => {
            if let Some(data) = resp.get("data") {
                // Try to format as a table if data contains an array of objects.
                if let Some(arr) = data.as_array()
                    && !arr.is_empty()
                    && arr.iter().all(|v| v.is_object())
                {
                    return format_table(arr);
                }
                // Otherwise, pretty-print the data.
                match serde_json::to_string_pretty(data) {
                    Ok(pretty) => pretty,
                    Err(_) => format!("{data}"),
                }
            } else {
                "OK".to_string()
            }
        }
        "error" => {
            let msg = resp
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            format!("\x1b[31mError: {msg}\x1b[0m")
        }
        _ => {
            // Unknown status; just pretty-print the whole thing.
            serde_json::to_string_pretty(resp).unwrap_or_else(|_| format!("{resp}"))
        }
    }
}

/// Format an array of JSON objects as a simple text table.
fn format_table(rows: &[serde_json::Value]) -> String {
    use std::collections::BTreeSet;
    use std::fmt::Write;

    // Collect all keys across all rows for stable column ordering.
    let mut columns = BTreeSet::new();
    for row in rows {
        if let Some(obj) = row.as_object() {
            for key in obj.keys() {
                columns.insert(key.clone());
            }
        }
    }
    let columns: Vec<String> = columns.into_iter().collect();

    if columns.is_empty() {
        return "[]".to_string();
    }

    // Compute column widths.
    let mut widths: Vec<usize> = columns.iter().map(|c| c.len()).collect();
    let cell_values: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            columns
                .iter()
                .enumerate()
                .map(|(ci, col)| {
                    let val = row.get(col).unwrap_or(&serde_json::Value::Null);
                    let s = if let Some(str_val) = val.as_str() {
                        str_val.to_string()
                    } else {
                        val.to_string()
                    };
                    if s.len() > widths[ci] {
                        widths[ci] = s.len();
                    }
                    s
                })
                .collect()
        })
        .collect();

    let mut output = String::new();

    // Header.
    let header: Vec<String> = columns
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{:width$}", c, width = widths[i]))
        .collect();
    let _ = writeln!(output, "| {} |", header.join(" | "));

    // Separator.
    let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
    let _ = writeln!(output, "|-{}-|", sep.join("-|-"));

    // Rows.
    for row_vals in &cell_values {
        let cells: Vec<String> = row_vals
            .iter()
            .enumerate()
            .map(|(i, v)| format!("{:width$}", v, width = widths[i]))
            .collect();
        let _ = writeln!(output, "| {} |", cells.join(" | "));
    }

    let _ = write!(output, "({} rows)", rows.len());
    output
}

// ---------------------------------------------------------------------------
// Status command
// ---------------------------------------------------------------------------

async fn run_status(address: &str) {
    println!("Server: {address}");

    match send_request(address, &serde_json::json!({"type": "Ping"})).await {
        Ok(resp) => {
            let status = resp
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            if status == "ok" {
                println!("Status: Connected");

                if let Some(data) = resp.get("data") {
                    if let Some(version) = data.get("version").and_then(|v| v.as_str()) {
                        println!("Version: {version}");
                    }
                    if let Some(pong) = data.get("pong").and_then(|v| v.as_bool()) {
                        println!("Ping/Pong: {pong}");
                    }
                    // Print any other fields the server returns.
                    if let Some(obj) = data.as_object() {
                        for (key, val) in obj {
                            if key != "version" && key != "pong" {
                                println!("{}: {}", capitalize(key), val);
                            }
                        }
                    }
                }
            } else {
                let msg = resp
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                eprintln!("Status: Error - {msg}");
            }
        }
        Err(e) => {
            eprintln!("Status: Unreachable");
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

/// Capitalize the first letter of a string.
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve {
            config,
            bind,
            port,
            grpc_port,
        } => {
            let mut cfg = load_config(&config);

            // CLI overrides.
            if let Some(b) = bind {
                cfg.server.bind_address = b;
            }
            if let Some(p) = port {
                cfg.server.port = p;
            }

            println!(
                "Starting AstraeaDB TCP  server on {}:{}",
                cfg.server.bind_address, cfg.server.port
            );
            println!(
                "Starting AstraeaDB gRPC server on {}:{}",
                cfg.server.bind_address, grpc_port
            );
            println!("Data directory: {}", cfg.storage.data_dir.display());
            println!("Buffer pool size: {} pages", cfg.storage.buffer_pool_size);

            // Create vector index with dimension and metric from config.
            // Defaults to 128-dim cosine when [vector] is omitted from the
            // config file, preserving back-compatibility.
            let metric = match parse_metric(&cfg.vector.metric) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("Configuration error: {e}");
                    std::process::exit(1);
                }
            };
            println!(
                "Vector index: {} dimensions, metric={}",
                cfg.vector.dimension, cfg.vector.metric
            );
            // Open the disk storage engine at the configured data dir. This
            // replays the WAL to rebuild in-memory indexes and returns the
            // highest node/edge ids so id allocation resumes correctly.
            let (storage, max_node_id, max_edge_id) =
                astraea_storage::DiskStorageEngine::open(&cfg.storage.data_dir)
                    .expect("Failed to open storage engine");
            if max_node_id > 0 || max_edge_id > 0 {
                println!(
                    "Recovered from WAL: next node_id={}, next edge_id={}",
                    max_node_id + 1,
                    max_edge_id + 1
                );
            }

            let mut graph_inner = astraea_graph::Graph::with_start_ids(
                Box::new(storage),
                max_node_id + 1,
                max_edge_id + 1,
            );

            // Load the persisted HNSW snapshot (or rebuild from storage when
            // the file is missing, corrupt, or has a dimension mismatch) and
            // attach it to the graph.  Delta-reconcile corrects any nodes
            // that were WAL-durable but not yet snapshotted, so vector search
            // is always consistent with storage after open.
            let hnsw_path = cfg.storage.data_dir.join("astraea.hnsw");
            match graph_inner.load_or_rebuild_vector_index(
                &hnsw_path,
                cfg.vector.dimension,
                metric,
            ) {
                Ok(astraea_graph::graph::VectorIndexInit::Loaded { inserted, removed }) => {
                    println!(
                        "Vector index loaded from snapshot: +{inserted} reconciled, \
                         -{removed} pruned"
                    );
                }
                Ok(astraea_graph::graph::VectorIndexInit::Rebuilt { count }) => {
                    println!(
                        "Vector index rebuilt from storage: {count} embeddings indexed"
                    );
                }
                Err(e) => {
                    eprintln!("Failed to initialize vector index: {e}");
                    std::process::exit(1);
                }
            }

            // Extract the attached index Arc for use in request handlers and
            // the shutdown snapshot.  load_or_rebuild_vector_index guarantees
            // the index is attached on Ok(_).
            let vector_index = graph_inner
                .vector_index()
                .expect("vector index must be attached after load_or_rebuild_vector_index")
                .clone();

            let graph: std::sync::Arc<dyn astraea_core::traits::GraphOps> =
                std::sync::Arc::new(graph_inner);

            // Build two handlers that share the same underlying graph.
            // AstraeaServer::new takes an owned RequestHandler (wraps it in
            // Arc internally), so we construct a separate one for TCP. The
            // gRPC server takes Arc<RequestHandler> directly.
            let vi: Option<std::sync::Arc<dyn astraea_core::traits::VectorIndex>> =
                Some(std::sync::Arc::clone(&vector_index));
            let tcp_handler =
                astraea_server::RequestHandler::new(std::sync::Arc::clone(&graph), vi.clone());
            let grpc_handler = std::sync::Arc::new(astraea_server::RequestHandler::new(
                std::sync::Arc::clone(&graph),
                vi,
            ));

            let server_config = astraea_server::ServerConfig {
                bind_address: cfg.server.bind_address.clone(),
                port: cfg.server.port,
                connection: astraea_server::ConnectionConfig::default(),
                tls: None, // TLS can be configured via TlsConfig if needed
            };
            let tcp_server = astraea_server::AstraeaServer::new(server_config, tcp_handler)
                .expect("Failed to create server");

            let grpc_bind = cfg.server.bind_address.clone();

            // Capture shared handles for the shutdown task.  Arc clones are
            // O(1) and don't touch the heap; PathBuf clone is a small alloc.
            let conn_mgr_for_shutdown = tcp_server.connection_manager().clone();
            let graph_for_shutdown = std::sync::Arc::clone(&graph);
            let vi_for_shutdown = std::sync::Arc::clone(&vector_index);
            let hnsw_path_for_shutdown = hnsw_path.clone();

            // Background task: wait for SIGTERM or SIGINT (Ctrl-C), flush
            // dirty buffer-pool pages to disk, then tell the TCP accept loop
            // to drain and exit.  The gRPC future is dropped when the select!
            // below resolves; individual RPC handlers have already completed
            // because they hold no long-lived borrows.
            //
            // astraeadb-issues.md #1 — signal handler for clean shutdown.
            tokio::spawn(async move {
                #[cfg(unix)]
                {
                    use tokio::signal::unix::{SignalKind, signal};
                    let mut sigterm = match signal(SignalKind::terminate()) {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("Failed to register SIGTERM handler: {e}");
                            return;
                        }
                    };
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => {
                            eprintln!("Received SIGINT, flushing storage and shutting down...");
                        }
                        _ = sigterm.recv() => {
                            eprintln!("Received SIGTERM, flushing storage and shutting down...");
                        }
                    }
                }
                #[cfg(not(unix))]
                {
                    if tokio::signal::ctrl_c().await.is_err() {
                        eprintln!("Ctrl-C handler registration failed");
                        return;
                    }
                    eprintln!("Received Ctrl-C, flushing storage and shutting down...");
                }

                // Flush dirty buffer-pool pages to disk before the process
                // exits.  WAL replay would also recover on next startup, but
                // flushing here makes the next cold-start faster and avoids
                // replaying records that are already on disk.
                if let Err(e) = graph_for_shutdown.flush() {
                    eprintln!("Storage flush failed during shutdown: {e}");
                } else {
                    eprintln!("Storage flushed successfully.");
                }

                // Persist the HNSW vector index snapshot so the next start
                // is a fast load + small delta-reconcile rather than a full
                // O(n·log n) rebuild.  This is best-effort: if it fails, the
                // WAL is the durability source of truth and the index will be
                // rebuilt from storage on the next open.  Write to a tmp file
                // first and rename atomically so a crash mid-save never leaves
                // a torn snapshot on disk.
                {
                    let mut tmp_name = hnsw_path_for_shutdown
                        .file_name()
                        .unwrap_or_default()
                        .to_os_string();
                    tmp_name.push(".tmp");
                    let tmp_path = hnsw_path_for_shutdown.with_file_name(tmp_name);
                    if let Err(e) = vi_for_shutdown.save_to_path(&tmp_path) {
                        eprintln!(
                            "Warning: vector index snapshot write failed: {e} \
                             (WAL is the source of truth; index will rebuild on next start)"
                        );
                    } else if let Err(e) = std::fs::rename(&tmp_path, &hnsw_path_for_shutdown) {
                        eprintln!(
                            "Warning: vector index snapshot rename failed: {e} \
                             (WAL is the source of truth; index will rebuild on next start)"
                        );
                    } else {
                        eprintln!("Vector index snapshot saved.");
                    }
                }

                // Signal the TCP accept loop to stop and drain in-flight
                // connections (wait_for_drain is called inside AstraeaServer::run).
                conn_mgr_for_shutdown.initiate_shutdown();
            });

            // Run both servers concurrently. If either exits, shut down.
            tokio::select! {
                result = tcp_server.run() => {
                    if let Err(e) = result {
                        eprintln!("TCP server error: {e}");
                        std::process::exit(1);
                    }
                }
                result = astraea_server::grpc::run_grpc_server(
                    grpc_bind,
                    grpc_port,
                    grpc_handler,
                ) => {
                    if let Err(e) = result {
                        eprintln!("gRPC server error: {e}");
                        std::process::exit(1);
                    }
                }
            }
        }

        Commands::Import {
            file,
            format,
            data_dir: _,
            address,
        } => {
            if format != "json" {
                eprintln!("Only JSON format is currently supported for import.");
                std::process::exit(1);
            }

            println!(
                "Importing from '{}' via server at {address}...",
                file.display()
            );

            if let Err(e) = run_import(&file, &address).await {
                eprintln!("Import failed: {e}");
                std::process::exit(1);
            }
        }

        Commands::Export {
            file,
            format,
            data_dir: _,
            address,
            max_id,
        } => {
            if format != "json" {
                eprintln!("Only JSON format is currently supported for export.");
                std::process::exit(1);
            }

            println!(
                "Exporting to '{}' via server at {address}...",
                file.display()
            );

            if let Err(e) = run_export(&file, &address, max_id).await {
                eprintln!("Export failed: {e}");
                std::process::exit(1);
            }
        }

        Commands::Shell { address } => {
            println!("Connecting to AstraeaDB at {address}...");
            // Run the shell in a blocking context since rustyline is synchronous.
            run_shell_blocking(address);
        }

        Commands::Status { address } => {
            run_status(&address).await;
        }

        Commands::Mcp {
            address,
            auth_token,
        } => {
            // MCP server: all user-visible output goes to stderr.
            // stdout is reserved for the JSON-RPC protocol.
            eprintln!("Starting AstraeaDB MCP server (proxy mode -> {address})");

            let config = astraea_mcp::McpConfig {
                address,
                auth_token,
            };
            let mut server = astraea_mcp::McpServer::new(config);
            let mut transport = astraea_mcp::transport::stdio::StdioTransport::new();

            if let Err(e) = server.run(&mut transport).await {
                eprintln!("MCP server error: {e}");
                std::process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- VectorConfig / parse_metric unit tests ---

    #[test]
    fn vector_config_defaults_to_128_cosine() {
        let cfg: Config = toml::from_str("").expect("empty TOML should parse");
        assert_eq!(cfg.vector.dimension, 128);
        assert_eq!(cfg.vector.metric, "cosine");
    }

    #[test]
    fn vector_config_omitted_block_defaults() {
        // A config with only a [server] block should still give VectorConfig defaults.
        let toml = r#"
[server]
port = 7687
"#;
        let cfg: Config = toml::from_str(toml).expect("should parse");
        assert_eq!(cfg.vector.dimension, 128);
        assert_eq!(cfg.vector.metric, "cosine");
    }

    #[test]
    fn vector_config_parses_dimension_768() {
        let toml = r#"
[vector]
dimension = 768
"#;
        let cfg: Config = toml::from_str(toml).expect("should parse");
        assert_eq!(cfg.vector.dimension, 768);
        assert_eq!(cfg.vector.metric, "cosine"); // metric defaults when omitted
    }

    #[test]
    fn vector_config_parses_dimension_and_metric() {
        let toml = r#"
[vector]
dimension = 1536
metric = "euclidean"
"#;
        let cfg: Config = toml::from_str(toml).expect("should parse");
        assert_eq!(cfg.vector.dimension, 1536);
        assert_eq!(cfg.vector.metric, "euclidean");
    }

    #[test]
    fn parse_metric_cosine() {
        assert_eq!(
            parse_metric("cosine").unwrap(),
            astraea_core::types::DistanceMetric::Cosine
        );
        // Case-insensitive.
        assert_eq!(
            parse_metric("Cosine").unwrap(),
            astraea_core::types::DistanceMetric::Cosine
        );
        assert_eq!(
            parse_metric("COSINE").unwrap(),
            astraea_core::types::DistanceMetric::Cosine
        );
    }

    #[test]
    fn parse_metric_euclidean() {
        assert_eq!(
            parse_metric("euclidean").unwrap(),
            astraea_core::types::DistanceMetric::Euclidean
        );
        assert_eq!(
            parse_metric("Euclidean").unwrap(),
            astraea_core::types::DistanceMetric::Euclidean
        );
    }

    #[test]
    fn parse_metric_dot_product() {
        assert_eq!(
            parse_metric("dot_product").unwrap(),
            astraea_core::types::DistanceMetric::DotProduct
        );
        // Short alias.
        assert_eq!(
            parse_metric("dot").unwrap(),
            astraea_core::types::DistanceMetric::DotProduct
        );
        assert_eq!(
            parse_metric("DOT").unwrap(),
            astraea_core::types::DistanceMetric::DotProduct
        );
    }

    #[test]
    fn parse_metric_unknown_returns_error() {
        let err = parse_metric("l2").unwrap_err();
        assert!(
            err.contains("Unknown vector metric"),
            "error should mention unknown metric, got: {err}"
        );
        assert!(err.contains("l2"), "error should echo the bad value");
    }
}
