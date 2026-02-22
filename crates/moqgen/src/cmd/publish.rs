use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use clap::Parser;
use moqgen_core::config::{OutputFormat, PayloadType, PublishConfig};
use moqgen_core::metrics::PublishMetrics;
use moqgen_core::publisher::PublisherWorker;

use crate::output::{print_json, print_publish_text, PublishReport};

#[derive(Parser, Debug)]
pub struct PublishArgs {
    /// Relay endpoint URL (e.g. https://localhost:4443)
    #[arg(long)]
    pub relay: url::Url,

    /// Broadcast path
    #[arg(long, default_value = "moqgen/test")]
    pub broadcast: String,

    /// Concurrent tracks per connection
    #[arg(long, default_value_t = 1)]
    pub tracks: usize,

    /// Groups per second per track
    #[arg(long, default_value_t = 30.0)]
    pub rate: f64,

    /// Frames per group
    #[arg(long, default_value_t = 1)]
    pub group_size: usize,

    /// Frame size in bytes
    #[arg(long, default_value_t = 1024)]
    pub frame_size: usize,

    /// Run duration in seconds
    #[arg(long, default_value_t = 10)]
    pub duration: u64,

    /// Concurrent QUIC connections (workers)
    #[arg(long, default_value_t = 1)]
    pub workers: usize,

    /// Skip TLS certificate verification
    #[arg(long)]
    pub insecure: bool,

    /// Output format
    #[arg(long, value_name = "text|json", default_value = "text")]
    pub output: String,

    /// Print stats every N seconds
    #[arg(long, default_value_t = 1)]
    pub metrics_interval: u64,

    /// Publish files from this directory; each file becomes one track
    /// named after its filename. Overrides --tracks.
    #[arg(long, value_name = "PATH")]
    pub static_dir: Option<PathBuf>,
}

pub async fn run(args: PublishArgs) -> anyhow::Result<()> {
    let output = parse_output_format(&args.output);

    let config = PublishConfig {
        relay: args.relay.clone(),
        broadcast: args.broadcast.clone(),
        tracks: args.tracks,
        rate: args.rate,
        group_size: args.group_size,
        frame_size: args.frame_size,
        duration_secs: args.duration,
        workers: args.workers,
        insecure: args.insecure,
        payload_type: PayloadType::Random,
        output: output.clone(),
        metrics_interval_secs: args.metrics_interval,
        static_dir: args.static_dir.clone(),
    };

    let metrics = Arc::new(PublishMetrics::default());

    // Periodic stats printer
    let metrics_clone = Arc::clone(&metrics);
    let output_clone = output.clone();
    let interval_secs = args.metrics_interval;
    let duration_secs = args.duration;
    let stats_handle = tokio::spawn(async move {
        let start = Instant::now();
        let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));
        ticker.tick().await; // skip first immediate tick
        loop {
            ticker.tick().await;
            let elapsed = start.elapsed().as_secs_f64();
            if elapsed > duration_secs as f64 + 1.0 {
                break;
            }
            let snap = metrics_clone.snapshot();
            let report = PublishReport::from_snapshot(&snap, elapsed);
            match output_clone {
                OutputFormat::Text => print_publish_text(&report),
                OutputFormat::Json => print_json(&report),
            }
        }
    });

    // Spawn one worker per QUIC connection
    let mut handles = Vec::with_capacity(config.workers);
    for _i in 0..config.workers {
        let worker = PublisherWorker::new(config.clone(), Arc::clone(&metrics));
        let url = args.relay.clone();
        handles.push(tokio::spawn(async move {
            worker.run(url).await
        }));
    }

    for handle in handles {
        handle
            .await
            .context("worker task panicked")?
            .context("publisher worker failed")?;
    }

    stats_handle.abort();

    // Print final summary
    let snap = metrics.snapshot();
    let report = PublishReport::from_snapshot(&snap, args.duration as f64);
    match output {
        OutputFormat::Text => {
            println!("\n--- Final publish stats ---");
            print_publish_text(&report);
        }
        OutputFormat::Json => print_json(&report),
    }

    Ok(())
}

pub fn parse_output_format(s: &str) -> OutputFormat {
    match s.to_lowercase().as_str() {
        "json" => OutputFormat::Json,
        _ => OutputFormat::Text,
    }
}
