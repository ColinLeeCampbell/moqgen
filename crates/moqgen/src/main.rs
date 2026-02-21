mod cmd;
mod output;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(
    name = "moqgen",
    version,
    about = "MoQ (Media over QUIC) load generator and probe — like telemetrygen for MoQ relays",
    long_about = None,
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Publish synthetic tracks to a relay
    Publish(cmd::publish::PublishArgs),
    /// Subscribe to tracks on a relay and report throughput
    Subscribe(cmd::subscribe::SubscribeArgs),
    /// Round-trip latency probe (publish + subscribe through a relay)
    Probe(cmd::probe::ProbeArgs),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialise tracing from RUST_LOG (default: warn)
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Publish(args) => cmd::publish::run(args)
            .await
            .context("publish command failed"),
        Command::Subscribe(args) => cmd::subscribe::run(args)
            .await
            .context("subscribe command failed"),
        Command::Probe(args) => cmd::probe::run(args)
            .await
            .context("probe command failed"),
    }
}
