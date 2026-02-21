use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tokio::time::Instant;
use tracing::{debug, warn};
use url::Url;

use crate::config::PublishConfig;
use crate::generator::FrameGenerator;
use crate::metrics::PublishMetrics;

pub struct PublisherWorker {
    pub config: PublishConfig,
    pub metrics: Arc<PublishMetrics>,
}

impl PublisherWorker {
    pub fn new(config: PublishConfig, metrics: Arc<PublishMetrics>) -> Self {
        Self { config, metrics }
    }

    pub async fn run(&self, url: Url) -> anyhow::Result<()> {
        // Create the local origin (what we publish)
        let origin = moq_lite::Origin::produce();

        // Get the consumer side BEFORE creating the broadcast, so it sees the announcement
        let origin_consumer = origin.consume();

        // Create and auto-publish our broadcast into the origin
        let mut broadcast = origin
            .create_broadcast(&self.config.broadcast)
            .with_context(|| format!("invalid broadcast path '{}'", self.config.broadcast))?;

        // Pre-create all tracks in the broadcast
        let mut track_producers: Vec<moq_lite::TrackProducer> =
            Vec::with_capacity(self.config.tracks);
        for i in 0..self.config.tracks {
            let track = moq_lite::Track::new(format!("track-{i}"));
            track_producers.push(broadcast.create_track(track));
        }

        // Connect to the relay and start the session
        let client = build_client(self.config.insecure)?
            .with_publish(origin_consumer);
        let _session = client
            .connect(url.clone())
            .await
            .with_context(|| format!("connect to {url}"))?;

        let deadline = Instant::now() + Duration::from_secs(self.config.duration_secs);
        let interval = Duration::from_secs_f64(1.0 / self.config.rate.max(f64::EPSILON));

        // Spawn one task per track
        let mut handles = Vec::with_capacity(track_producers.len());
        for mut track_producer in track_producers {
            let metrics = Arc::clone(&self.metrics);
            let config = self.config.clone();

            handles.push(tokio::spawn(async move {
                let mut generator =
                    FrameGenerator::new(config.frame_size, config.payload_type.clone());

                loop {
                    if Instant::now() >= deadline {
                        break;
                    }

                    let mut group = track_producer.append_group();
                    for _ in 0..config.group_size {
                        let frame = generator.generate();
                        let frame_len = frame.len() as u64;
                        group.write_frame(frame);
                        metrics.frames_total.fetch_add(1, Ordering::Relaxed);
                        metrics.bytes_total.fetch_add(frame_len, Ordering::Relaxed);
                    }
                    group.close();
                    metrics.groups_total.fetch_add(1, Ordering::Relaxed);

                    tokio::time::sleep(interval).await;
                }
            }));
        }

        for handle in handles {
            if let Err(e) = handle.await {
                warn!("track task panicked: {e}");
                self.metrics.errors_total.fetch_add(1, Ordering::Relaxed);
            }
        }

        // Dropping _session here closes the connection cleanly
        drop(_session);

        debug!("publisher done for {url}");
        Ok(())
    }
}

/// Build a moq-native QUIC client with optional TLS verification disabled.
pub fn build_client(insecure: bool) -> anyhow::Result<moq_native::Client> {
    let mut config = moq_native::ClientConfig::default();
    if insecure {
        config.tls.disable_verify = Some(true);
    }
    moq_native::Client::new(config).context("build QUIC client")
}
