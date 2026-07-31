//! `stumble-api` binary, installed with the rest of Stumble via
//! `cargo install --path crates/stumble-cli`.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    stumble_api::run().await
}
