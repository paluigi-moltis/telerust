use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    telerust::cli::run().await
}
