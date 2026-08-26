//! The `astraeadb` binary.
//!
//! Identical to the one shipped by the `astraea-cli` package: both are thin
//! `main` functions over [`astraea_cli::run`]. This package exists so that
//! `cargo install astraeadb`, the command people actually guess, works.

#[tokio::main]
async fn main() {
    astraea_cli::run().await
}
