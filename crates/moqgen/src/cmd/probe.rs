use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use clap::Parser;
use moqgen_core::config::{OutputFormat, ProbeConfig};
use moqgen_core::metrics::{LatencyHistogram, PublishMetrics, SubscribeMetrics};
use moqgen_core::probe::ProbeWorker;
use tokio_util::sync::CancellationToken;

use crate::cmd::publish::parse_output_format;
use crate::output::{
    print_json, print_latency_text, print_publish_text, print_subscribe_text, ProbeReport,
    PublishReport, SubscribeReport,
};

#[derive(Parser, Debug)]
pub struct ProbeArgs {
    /// Relay endpoint URL
    #[arg(long)]
    pub relay: url::Url,

    /// Broadcast path
    #[arg(long, default_value = "moqgen/test")]
    pub broadcast: String,

    /// Concurrent tracks
    #[arg(long, default_value_t = 1)]
    pub tracks: usize,

    /// Groups per second per track
    #[arg(long, default_value_t = 10.0)]
    pub rate: f64,

    /// Frame size in bytes (minimum 8 for timestamp)
    #[arg(long, default_value_t = 64)]
    pub frame_size: usize,

    /// Run duration in seconds
    #[arg(long, default_value_t = 10)]
    pub duration: u64,

    /// Skip TLS certificate verification
    #[arg(long)]
    pub insecure: bool,

    /// Output format
    #[arg(long, value_name = "text|json", default_value = "text")]
    pub output: String,

    /// Print stats every N seconds
    #[arg(long, default_value_t = 1)]
    pub metrics_interval: u64,
}

pub async fn run(args: ProbeArgs, cancel: CancellationToken) -> anyhow::Result<()> {
    let output = parse_output_format(&args.output);

    // Enforce minimum frame size for timestamp
    let frame_size = args.frame_size.max(8);

    let config = ProbeConfig {
        relay: args.relay.clone(),
        broadcast: args.broadcast.clone(),
        tracks: args.tracks,
        rate: args.rate,
        frame_size,
        duration_secs: args.duration,
        insecure: args.insecure,
        output: output.clone(),
        metrics_interval_secs: args.metrics_interval,
    };

    let pub_metrics = Arc::new(PublishMetrics::default());
    let sub_metrics = Arc::new(SubscribeMetrics::default());
    let latency = Arc::new(LatencyHistogram::new().context("latency histogram init")?);

    // Periodic stats printer
    let pub_m = Arc::clone(&pub_metrics);
    let sub_m = Arc::clone(&sub_metrics);
    let lat_h = Arc::clone(&latency);
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
            let pub_snap = pub_m.snapshot();
            let sub_snap = sub_m.snapshot();
            let pub_report = PublishReport::from_snapshot(&pub_snap, elapsed);
            let sub_report = SubscribeReport::from_snapshot(&sub_snap, elapsed);

            match output_clone {
                OutputFormat::Text => {
                    print_publish_text(&pub_report);
                    print_subscribe_text(&sub_report);
                    if let Some(lat_snap) = lat_h.snapshot() {
                        let probe_report =
                            ProbeReport::new(pub_report, sub_report, &lat_snap, elapsed);
                        print_latency_text(&probe_report);
                    }
                    println!("---");
                }
                OutputFormat::Json => {
                    if let Some(lat_snap) = lat_h.snapshot() {
                        let probe_report =
                            ProbeReport::new(pub_report, sub_report, &lat_snap, elapsed);
                        print_json(&probe_report);
                    }
                }
            }
        }
    });

    let worker = ProbeWorker::new(
        config,
        Arc::clone(&pub_metrics),
        Arc::clone(&sub_metrics),
        Arc::clone(&latency),
        cancel.clone(),
    );
    worker
        .run(args.relay.clone())
        .await
        .context("probe worker failed")?;

    cancel.cancel();
    let _ = stats_handle.await;

    // Final summary
    let elapsed = args.duration as f64;
    let pub_snap = pub_metrics.snapshot();
    let sub_snap = sub_metrics.snapshot();
    let pub_report = PublishReport::from_snapshot(&pub_snap, elapsed);
    let sub_report = SubscribeReport::from_snapshot(&sub_snap, elapsed);

    match output {
        OutputFormat::Text => {
            println!("\n--- Final probe stats ---");
            print_publish_text(&pub_report);
            print_subscribe_text(&sub_report);
            if let Some(lat_snap) = latency.snapshot() {
                let probe_report = ProbeReport::new(pub_report, sub_report, &lat_snap, elapsed);
                print_latency_text(&probe_report);
            } else {
                println!("latency | no samples collected");
            }
        }
        OutputFormat::Json => {
            if let Some(lat_snap) = latency.snapshot() {
                let probe_report = ProbeReport::new(pub_report, sub_report, &lat_snap, elapsed);
                print_json(&probe_report);
            }
        }
    }

    Ok(())
}
