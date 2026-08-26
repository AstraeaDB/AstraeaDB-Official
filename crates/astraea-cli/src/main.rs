//! The `astraeadb` binary, as shipped by the `astraea-cli` package.
//!
//! Everything lives in the library so the identical binary can also be shipped
//! by the `astraeadb` package; see `astraea_cli::run`.

#[tokio::main]
async fn main() {
    astraea_cli::run().await
}
