use loco_rs::cli;
use migration::Migrator;
use sagan::app::App;

#[tokio::main]
async fn main() -> loco_rs::Result<()> {
    cli::main::<App, Migrator>().await
}
