mod cmd;
mod output;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tokio_util::sync::CancellationToken;
use tracing::info;
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
    // Initialise tracing from RUST_LOG.
    // Default: warn for moqgen crates, error for moq-lite/moq-native (suppresses
    // noisy transport-closed warnings on shutdown).
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                EnvFilter::new("warn,moq_lite=error,moq_native=error,web_transport_quinn=error")
            }),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    let cancel = CancellationToken::new();

    // Spawn Ctrl+C handler
    let cancel_for_signal = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            info!("received Ctrl+C, shutting down gracefully...");
            cancel_for_signal.cancel();
        }
    });

    match cli.command {
        Command::Publish(args) => cmd::publish::run(args, cancel)
            .await
            .context("publish command failed"),
        Command::Subscribe(args) => cmd::subscribe::run(args, cancel)
            .await
            .context("subscribe command failed"),
        Command::Probe(args) => cmd::probe::run(args, cancel)
            .await
            .context("probe command failed"),
    }
}
