use std::io::Write;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use url::Url;

use crate::config::SubscribeConfig;
use crate::generator::read_timestamp_us;
use crate::metrics::{LatencyHistogram, SubscribeMetrics};
use crate::publisher::build_client;
use crate::static_files::list_static_dir;

pub struct SubscriberWorker {
    pub config: SubscribeConfig,
    pub metrics: Arc<SubscribeMetrics>,
    /// Optional latency histogram — populated when used from probe mode.
    pub latency: Option<Arc<LatencyHistogram>>,
    pub cancel: CancellationToken,
}

impl SubscriberWorker {
    pub fn new(config: SubscribeConfig, metrics: Arc<SubscribeMetrics>, cancel: CancellationToken) -> Self {
        Self {
            config,
            metrics,
            latency: None,
            cancel,
        }
    }

    pub fn with_latency(mut self, histogram: Arc<LatencyHistogram>) -> Self {
        self.latency = Some(histogram);
        self
    }

    pub async fn run(&self, url: Url) -> anyhow::Result<()> {
        // Create a fresh origin for incoming (subscribed) broadcasts.
        // The session will publish remote announcements into the OriginProducer.
        let sub_origin = moq_lite::Origin::produce();

        // Get the consumer side BEFORE giving the producer to the session.
        // Messages sent to the producer after consumer creation are buffered
        // and received on the consumer.
        let mut origin_consumer = sub_origin.consume();

        // Connect with subscribe-only role
        let client = build_client(self.config.insecure)?.with_consume(sub_origin);
        let _session = client
            .connect(url.clone())
            .await
            .with_context(|| format!("connect to {url}"))?;

        let deadline = tokio::time::Instant::now()
            + Duration::from_secs(self.config.duration_secs);

        // Wait for our target broadcast to be announced by the remote
        let broadcast_consumer = tokio::select! {
            result = tokio::time::timeout(
                Duration::from_secs(self.config.duration_secs),
                find_broadcast(&mut origin_consumer, &self.config.broadcast),
            ) => {
                result
                    .context("timed out waiting for broadcast announcement")?
                    .context("broadcast not found (connection dropped)")?
            }
            _ = self.cancel.cancelled() => {
                debug!("subscriber cancelled while waiting for broadcast");
                drop(_session);
                return Ok(());
            }
        };

        info!("subscribed to broadcast '{}'", self.config.broadcast);

        // Resolve track names: from static dir filenames or synthetic "track-{i}"
        let track_names: Vec<String> = if let Some(ref dir) = self.config.static_dir {
            list_static_dir(dir)
                .with_context(|| format!("list static dir '{}'", dir.display()))?
        } else {
            (0..self.config.tracks.max(1))
                .map(|i| format!("track-{i}"))
                .collect()
        };

        // Ensure output directory exists if configured
        if let Some(ref out_dir) = self.config.output_dir {
            std::fs::create_dir_all(out_dir)
                .with_context(|| format!("create output dir '{}'", out_dir.display()))?;
        }

        let mut handles = Vec::new();
        for track_name in track_names {
            let track_info = moq_lite::Track::new(track_name.clone());
            let mut track_consumer = broadcast_consumer.subscribe_track(&track_info);
            let metrics = Arc::clone(&self.metrics);
            let latency = self.latency.clone();
            let validate = self.config.validate;
            let expected_frame_size = self.config.frame_size;
            let output_dir = self.config.output_dir.clone();
            let cancel = self.cancel.clone();

            handles.push(tokio::spawn(async move {
                let mut last_seq: Option<u64> = None;

                // Open file once before the group loop so all groups append
                let mut file_writer = if let Some(ref out_dir) = output_dir {
                    let out_path = out_dir.join(&track_name);
                    match std::fs::File::create(&out_path) {
                        Ok(f) => Some(std::io::BufWriter::new(f)),
                        Err(e) => {
                            warn!("failed to create '{}': {e}", out_path.display());
                            metrics.errors_total.fetch_add(1, Ordering::Relaxed);
                            None
                        }
                    }
                } else {
                    None
                };

                loop {
                    if tokio::time::Instant::now() >= deadline || cancel.is_cancelled() {
                        break;
                    }

                    let mut group = match tokio::select! {
                        result = track_consumer.next_group() => result,
                        _ = cancel.cancelled() => break,
                    } {
                        Ok(Some(g)) => g,
                        Ok(None) => {
                            debug!("track '{track_name}' closed cleanly");
                            break;
                        }
                        Err(e) => {
                            if cancel.is_cancelled() {
                                debug!("track '{track_name}' closed during shutdown");
                            } else {
                                warn!("track '{track_name}' error: {e}");
                                metrics.errors_total.fetch_add(1, Ordering::Relaxed);
                            }
                            break;
                        }
                    };

                    let seq = group.info.sequence;

                    if validate {
                        if let Some(prev) = last_seq {
                            if seq <= prev {
                                warn!("track '{track_name}': seq reuse: got {seq} after {prev}");
                                metrics.errors_total.fetch_add(1, Ordering::Relaxed);
                            } else if seq > prev + 1 {
                                let dropped = seq - prev - 1;
                                metrics
                                    .groups_dropped
                                    .fetch_add(dropped, Ordering::Relaxed);
                            }
                        }
                    }
                    last_seq = Some(seq);
                    metrics.groups_total.fetch_add(1, Ordering::Relaxed);

                    loop {
                        let frame = match group.read_frame().await {
                            Ok(Some(f)) => f,
                            Ok(None) => break,
                            Err(e) => {
                                warn!("frame read error on '{track_name}': {e}");
                                metrics.errors_total.fetch_add(1, Ordering::Relaxed);
                                break;
                            }
                        };

                        // Latency measurement (probe mode)
                        if let Some(ref hist) = latency {
                            if let Some(sent_us) = read_timestamp_us(&frame) {
                                let now_us = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_micros() as u64;
                                if now_us >= sent_us {
                                    hist.record(now_us - sent_us);
                                }
                            }
                        }

                        if validate && expected_frame_size > 0
                            && frame.len() != expected_frame_size
                        {
                            warn!(
                                "frame size mismatch on '{track_name}': expected {expected_frame_size}, got {}",
                                frame.len()
                            );
                            metrics.errors_total.fetch_add(1, Ordering::Relaxed);
                        }

                        let frame_len = frame.len() as u64;
                        metrics.frames_total.fetch_add(1, Ordering::Relaxed);
                        metrics.bytes_total.fetch_add(frame_len, Ordering::Relaxed);

                        // Stream frame directly to disk
                        if let Some(ref mut writer) = file_writer {
                            if let Err(e) = writer.write_all(&frame) {
                                warn!("failed to write frame for '{track_name}': {e}");
                                metrics.errors_total.fetch_add(1, Ordering::Relaxed);
                                file_writer = None; // stop trying to write
                            }
                        }
                    }
                }

                // Flush remaining buffered data to disk
                if let Some(mut writer) = file_writer {
                    if let Err(e) = writer.flush() {
                        warn!("failed to flush file for '{track_name}': {e}");
                        metrics.errors_total.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }));
        }

        for handle in handles {
            if let Err(e) = handle.await {
                warn!("subscriber track task panicked: {e}");
            }
        }

        drop(_session);
        Ok(())
    }
}

/// Wait for a specific broadcast path to be announced.
async fn find_broadcast(
    consumer: &mut moq_lite::OriginConsumer,
    path: &str,
) -> Option<moq_lite::BroadcastConsumer> {
    loop {
        match consumer.announced().await {
            Some((announced_path, Some(broadcast))) if announced_path.as_str() == path => {
                return Some(broadcast);
            }
            Some(_) => {
                // Different path or unannounce — keep waiting
            }
            None => return None,
        }
    }
}
