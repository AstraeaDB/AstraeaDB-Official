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

        /// Port (overrides config file).
        #[arg(short, long)]
        port: Option<u16>,
    },

    /// Import data from a file.
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
    },

    /// Export data to a file.
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

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { config, bind, port } => {
            let mut cfg = load_config(&config);

            // CLI overrides
            if let Some(b) = bind {
                cfg.server.bind_address = b;
            }
            if let Some(p) = port {
                cfg.server.port = p;
            }

            println!(
                "Starting AstraeaDB server on {}:{}",
                cfg.server.bind_address, cfg.server.port
            );
            println!("Data directory: {}", cfg.storage.data_dir.display());
            println!("Buffer pool size: {} pages", cfg.storage.buffer_pool_size);

            // Create the in-memory graph for now (will use DiskStorageEngine later).
            let storage = astraea_graph::test_utils::InMemoryStorage::new();
            let graph = astraea_graph::Graph::new(Box::new(storage));
            let graph = std::sync::Arc::new(graph);

            let handler = astraea_server::RequestHandler::new(graph);
            let server_config = astraea_server::ServerConfig {
                bind_address: cfg.server.bind_address,
                port: cfg.server.port,
            };
            let server = astraea_server::AstraeaServer::new(server_config, handler);

            if let Err(e) = server.run().await {
                eprintln!("Server error: {e}");
                std::process::exit(1);
            }
        }

        Commands::Shell { address } => {
            println!("Connecting to AstraeaDB at {address}...");
            run_shell(&address).await;
        }

        Commands::Status { address } => {
            println!("Checking AstraeaDB status at {address}...");
            match send_request(&address, r#"{"type":"Ping"}"#).await {
                Ok(response) => println!("Server response: {response}"),
                Err(e) => {
                    eprintln!("Failed to connect: {e}");
                    std::process::exit(1);
                }
            }
        }

        Commands::Import {
            file,
            format,
            data_dir,
        } => {
            println!(
                "Importing from {} (format: {format}) into {}",
                file.display(),
                data_dir.display()
            );
            eprintln!("Import not yet implemented");
            std::process::exit(1);
        }

        Commands::Export {
            file,
            format,
            data_dir,
        } => {
            println!(
                "Exporting to {} (format: {format}) from {}",
                file.display(),
                data_dir.display()
            );
            eprintln!("Export not yet implemented");
            std::process::exit(1);
        }
    }
}

async fn send_request(address: &str, json_line: &str) -> Result<String, Box<dyn std::error::Error>> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    let mut stream = TcpStream::connect(address).await?;
    let (reader, mut writer) = stream.split();

    let mut msg = json_line.to_string();
    msg.push('\n');
    writer.write_all(msg.as_bytes()).await?;

    let mut reader = BufReader::new(reader);
    let mut response = String::new();
    reader.read_line(&mut response).await?;
    Ok(response.trim().to_string())
}

async fn run_shell(address: &str) {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    println!("AstraeaDB Shell. Type JSON requests, one per line. Ctrl-D to exit.");
    print!("> ");

    while let Ok(Some(line)) = lines.next_line().await {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            print!("> ");
            continue;
        }

        match send_request(address, trimmed).await {
            Ok(response) => println!("{response}"),
            Err(e) => eprintln!("Error: {e}"),
        }
        print!("> ");
    }

    println!("Bye.");
}
