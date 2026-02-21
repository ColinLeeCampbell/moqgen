use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tracing::{debug, info, warn};
use url::Url;

use crate::config::SubscribeConfig;
use crate::generator::read_timestamp_us;
use crate::metrics::{LatencyHistogram, SubscribeMetrics};
use crate::publisher::build_client;

pub struct SubscriberWorker {
    pub config: SubscribeConfig,
    pub metrics: Arc<SubscribeMetrics>,
    /// Optional latency histogram — populated when used from probe mode.
    pub latency: Option<Arc<LatencyHistogram>>,
}

impl SubscriberWorker {
    pub fn new(config: SubscribeConfig, metrics: Arc<SubscribeMetrics>) -> Self {
        Self {
            config,
            metrics,
            latency: None,
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
        let broadcast_consumer = tokio::time::timeout(
            Duration::from_secs(self.config.duration_secs),
            find_broadcast(&mut origin_consumer, &self.config.broadcast),
        )
        .await
        .context("timed out waiting for broadcast announcement")?
        .context("broadcast not found (connection dropped)")?;

        info!("subscribed to broadcast '{}'", self.config.broadcast);

        let track_names = if self.config.track_names.is_empty() {
            vec!["track-0".to_string()]
        } else {
            self.config.track_names.clone()
        };

        let mut handles = Vec::new();
        for track_name in track_names {
            let track_info = moq_lite::Track::new(track_name.clone());
            let mut track_consumer = broadcast_consumer.subscribe_track(&track_info);
            let metrics = Arc::clone(&self.metrics);
            let latency = self.latency.clone();
            let validate = self.config.validate;
            let expected_frame_size = self.config.frame_size;

            handles.push(tokio::spawn(async move {
                let mut last_seq: Option<u64> = None;

                loop {
                    if tokio::time::Instant::now() >= deadline {
                        break;
                    }

                    let mut group = match track_consumer.next_group().await {
                        Ok(Some(g)) => g,
                        Ok(None) => {
                            debug!("track '{track_name}' closed cleanly");
                            break;
                        }
                        Err(e) => {
                            warn!("track '{track_name}' error: {e}");
                            metrics.errors_total.fetch_add(1, Ordering::Relaxed);
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
