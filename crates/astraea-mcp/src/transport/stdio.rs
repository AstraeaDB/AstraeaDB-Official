use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Stdin, Stdout};

use super::Transport;

/// MCP transport over stdin/stdout using newline-delimited JSON.
///
/// All logging MUST go to stderr — stdout is the MCP protocol channel.
pub struct StdioTransport {
    reader: BufReader<Stdin>,
    writer: Stdout,
}

impl StdioTransport {
    pub fn new() -> Self {
        Self {
            reader: BufReader::new(tokio::io::stdin()),
            writer: tokio::io::stdout(),
        }
    }
}

impl Transport for StdioTransport {
    async fn read_message(&mut self) -> std::io::Result<Option<String>> {
        let mut line = String::new();
        let bytes_read = self.reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            return Ok(None); // EOF
        }
        Ok(Some(line))
    }

    async fn write_message(&mut self, message: &str) -> std::io::Result<()> {
        self.writer.write_all(message.as_bytes()).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;
        Ok(())
    }
}
