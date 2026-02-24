use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use clap::Parser;
use moqgen_core::config::{OutputFormat, SubscribeConfig};
use moqgen_core::metrics::SubscribeMetrics;
use moqgen_core::subscriber::SubscriberWorker;
use tokio_util::sync::CancellationToken;

use crate::cmd::publish::parse_output_format;
use crate::output::{print_json, print_subscribe_text, SubscribeReport};

#[derive(Parser, Debug)]
pub struct SubscribeArgs {
    /// Relay endpoint URL
    #[arg(long)]
    pub relay: url::Url,

    /// Broadcast path to subscribe to
    #[arg(long, default_value = "moqgen/test")]
    pub broadcast: String,

    /// Number of tracks to subscribe to (track-0 .. track-N-1)
    #[arg(long, default_value_t = 1)]
    pub tracks: usize,

    /// Run duration in seconds
    #[arg(long, default_value_t = 10)]
    pub duration: u64,

    /// Enable sequence validation (gap/duplicate detection + frame size check)
    #[arg(long)]
    pub validate: bool,

    /// Expected frame size for validation (0 = skip)
    #[arg(long, default_value_t = 0)]
    pub frame_size: usize,

    /// Skip TLS certificate verification
    #[arg(long)]
    pub insecure: bool,

    /// Output format
    #[arg(long, value_name = "text|json", default_value = "text")]
    pub output: String,

    /// Print stats every N seconds
    #[arg(long, default_value_t = 1)]
    pub metrics_interval: u64,

    /// Discover track names from filenames in this directory.
    #[arg(long, value_name = "PATH")]
    pub static_dir: Option<PathBuf>,

    /// Write each received track to a file in this directory.
    #[arg(long, value_name = "PATH")]
    pub output_dir: Option<PathBuf>,
}

pub async fn run(args: SubscribeArgs, cancel: CancellationToken) -> anyhow::Result<()> {
    let output = parse_output_format(&args.output);

    let config = SubscribeConfig {
        relay: args.relay.clone(),
        broadcast: args.broadcast.clone(),
        tracks: args.tracks,
        duration_secs: args.duration,
        validate: args.validate,
        insecure: args.insecure,
        output: output.clone(),
        metrics_interval_secs: args.metrics_interval,
        frame_size: args.frame_size,
        static_dir: args.static_dir.clone(),
        output_dir: args.output_dir.clone(),
    };

    let metrics = Arc::new(SubscribeMetrics::default());

    // Periodic stats printer
    let metrics_clone = Arc::clone(&metrics);
    let output_clone = output.clone();
    let interval_secs = args.metrics_interval;
    let duration_secs = args.duration;
    let stats_cancel = cancel.clone();
    let stats_handle = tokio::spawn(async move {
        let start = Instant::now();
        let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));
        ticker.tick().await;
        loop {
            tokio::select! {
                _ = ticker.tick() => {}
                _ = stats_cancel.cancelled() => break,
            }
            let elapsed = start.elapsed().as_secs_f64();
            if elapsed > duration_secs as f64 + 1.0 {
                break;
            }
            let snap = metrics_clone.snapshot();
            let report = SubscribeReport::from_snapshot(&snap, elapsed);
            match output_clone {
                OutputFormat::Text => print_subscribe_text(&report),
                OutputFormat::Json => print_json(&report),
            }
        }
    });

    let worker = SubscriberWorker::new(config, Arc::clone(&metrics), cancel.clone());
    worker
        .run(args.relay.clone())
        .await
        .context("subscriber worker failed")?;

    cancel.cancel();
    let _ = stats_handle.await;

    // Final summary
    let snap = metrics.snapshot();
    let report = SubscribeReport::from_snapshot(&snap, args.duration as f64);
    match output {
        OutputFormat::Text => {
            println!("\n--- Final subscribe stats ---");
            print_subscribe_text(&report);
        }
        OutputFormat::Json => print_json(&report),
    }

    Ok(())
}
