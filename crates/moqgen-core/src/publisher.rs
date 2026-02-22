use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use bytes::Bytes;
use tokio::time::Instant;
use tracing::{debug, warn};
use url::Url;

use crate::config::PublishConfig;
use crate::generator::FrameGenerator;
use crate::metrics::PublishMetrics;
use crate::static_files::load_static_dir;

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

        // Build (track_name, file_bytes_or_None) pairs depending on mode
        let track_entries: Vec<(String, Option<Bytes>)> = if let Some(ref dir) = self.config.static_dir {
            let files = load_static_dir(dir)
                .with_context(|| format!("load static dir '{}'", dir.display()))?;
            files.into_iter().map(|(name, bytes)| (name, Some(bytes))).collect()
        } else {
            (0..self.config.tracks)
                .map(|i| (format!("track-{i}"), None))
                .collect()
        };

        // Pre-create all tracks in the broadcast
        let mut track_producers: Vec<(String, Option<Bytes>, moq_lite::TrackProducer)> =
            Vec::with_capacity(track_entries.len());
        for (name, bytes) in track_entries {
            let track = moq_lite::Track::new(name.clone());
            let producer = broadcast.create_track(track);
            track_producers.push((name, bytes, producer));
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
        for (_track_name, file_bytes, mut track_producer) in track_producers {
            let metrics = Arc::clone(&self.metrics);
            let config = self.config.clone();

            handles.push(tokio::spawn(async move {
                if let Some(file_bytes) = file_bytes {
                    // Static mode: loop sending file content chunked into frame_size groups
                    loop {
                        if Instant::now() >= deadline {
                            break;
                        }

                        let mut group = track_producer.append_group();
                        for chunk in file_bytes.chunks(config.frame_size.max(1)) {
                            let frame = Bytes::copy_from_slice(chunk);
                            let frame_len = frame.len() as u64;
                            group.write_frame(frame);
                            metrics.frames_total.fetch_add(1, Ordering::Relaxed);
                            metrics.bytes_total.fetch_add(frame_len, Ordering::Relaxed);
                        }
                        group.close();
                        metrics.groups_total.fetch_add(1, Ordering::Relaxed);

                        tokio::time::sleep(interval).await;
                    }
                } else {
                    // Synthetic mode: existing behaviour
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
