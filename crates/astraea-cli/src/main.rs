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
}

/// Configuration file structure.
#[derive(Debug, Deserialize)]
struct Config {
    #[serde(default)]
    server: ServerConfig,
    #[serde(default)]
    storage: StorageConfig,
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

fn load_config(path: &PathBuf) -> Config {
    match std::fs::read_to_string(path) {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(config) => config,
            Err(e) => {
                eprintln!("Warning: failed to parse config file: {e}");
                Config {
                    server: ServerConfig::default(),
                    storage: StorageConfig::default(),
                }
            }
        },
        Err(_) => Config {
            server: ServerConfig::default(),
            storage: StorageConfig::default(),
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
                if let Some(emb) = embedding {
                    if !emb.is_null() {
                        req["embedding"] = emb;
                    }
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
                if status == "ok" {
                    if let Some(data) = resp.get("data") {
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
                if status == "ok" {
                    if let Some(data) = resp.get("data") {
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
    use rustyline::error::ReadlineError;
    use rustyline::DefaultEditor;

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

    match rt.block_on(send_request(
        &address,
        &serde_json::json!({"type": "Ping"}),
    )) {
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
                if let Some(arr) = data.as_array() {
                    if !arr.is_empty() && arr.iter().all(|v| v.is_object()) {
                        return format_table(arr);
                    }
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

            // Create vector index (128-dim cosine by default).
            let vector_index = std::sync::Arc::new(
                astraea_vector::HnswVectorIndex::new(128, astraea_core::types::DistanceMetric::Cosine),
            );

            // Create the in-memory graph with vector index (will use DiskStorageEngine later).
            let storage = astraea_graph::test_utils::InMemoryStorage::new();
            let graph = astraea_graph::Graph::with_vector_index(
                Box::new(storage),
                std::sync::Arc::clone(&vector_index) as std::sync::Arc<dyn astraea_core::traits::VectorIndex>,
            );
            let graph: std::sync::Arc<dyn astraea_core::traits::GraphOps> =
                std::sync::Arc::new(graph);

            // Build two handlers that share the same underlying graph.
            // AstraeaServer::new takes an owned RequestHandler (wraps it in
            // Arc internally), so we construct a separate one for TCP. The
            // gRPC server takes Arc<RequestHandler> directly.
            let vi: Option<std::sync::Arc<dyn astraea_core::traits::VectorIndex>> =
                Some(std::sync::Arc::clone(&vector_index) as std::sync::Arc<dyn astraea_core::traits::VectorIndex>);
            let tcp_handler =
                astraea_server::RequestHandler::new(std::sync::Arc::clone(&graph), vi.clone());
            let grpc_handler = std::sync::Arc::new(
                astraea_server::RequestHandler::new(std::sync::Arc::clone(&graph), vi),
            );

            let server_config = astraea_server::ServerConfig {
                bind_address: cfg.server.bind_address.clone(),
                port: cfg.server.port,
            };
            let tcp_server =
                astraea_server::AstraeaServer::new(server_config, tcp_handler);

            let grpc_bind = cfg.server.bind_address.clone();

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
    }
}
