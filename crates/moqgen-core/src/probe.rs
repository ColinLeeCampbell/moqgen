use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tracing::debug;
use url::Url;

use crate::config::{OutputFormat, ProbeConfig, SubscribeConfig};
use crate::generator::FrameGenerator;
use crate::metrics::{LatencyHistogram, PublishMetrics, SubscribeMetrics};
use crate::publisher::build_client;
use crate::subscriber::SubscriberWorker;

pub struct ProbeWorker {
    pub config: ProbeConfig,
    pub pub_metrics: Arc<PublishMetrics>,
    pub sub_metrics: Arc<SubscribeMetrics>,
    pub latency: Arc<LatencyHistogram>,
}

impl ProbeWorker {
    pub fn new(
        config: ProbeConfig,
        pub_metrics: Arc<PublishMetrics>,
        sub_metrics: Arc<SubscribeMetrics>,
        latency: Arc<LatencyHistogram>,
    ) -> Self {
        Self {
            config,
            pub_metrics,
            sub_metrics,
            latency,
        }
    }

    pub async fn run(&self, url: Url) -> anyhow::Result<()> {
        let frame_size = self.config.frame_size.max(8);
        let deadline =
            tokio::time::Instant::now() + Duration::from_secs(self.config.duration_secs);
        let interval = Duration::from_secs_f64(1.0 / self.config.rate.max(f64::EPSILON));

        // --- Publisher task (inline, uses timestamped frames) ---
        let pub_url = url.clone();
        let pub_config = self.config.clone();
        let pub_metrics = Arc::clone(&self.pub_metrics);
        let pub_insecure = self.config.insecure;

        let pub_handle = tokio::spawn(async move {
            let result: anyhow::Result<()> = async {
                let origin = moq_lite::Origin::produce();
                let origin_consumer = origin.consume();
                let mut broadcast = origin
                    .create_broadcast(&pub_config.broadcast)
                    .with_context(|| {
                        format!("invalid broadcast path '{}'", pub_config.broadcast)
                    })?;

                let mut track_producers: Vec<moq_lite::TrackProducer> = (0..pub_config.tracks)
                    .map(|i| broadcast.create_track(moq_lite::Track::new(format!("track-{i}"))))
                    .collect();

                let client = build_client(pub_insecure)?.with_publish(origin_consumer);
                let _session = client
                    .connect(pub_url.clone())
                    .await
                    .with_context(|| format!("connect to {pub_url}"))?;

                let mut handles = Vec::new();
                for mut track_producer in track_producers.drain(..) {
                    let pm = Arc::clone(&pub_metrics);
                    let fs = frame_size;
                    handles.push(tokio::spawn(async move {
                        let mut gen = FrameGenerator::new(fs, crate::config::PayloadType::Random);
                        loop {
                            if tokio::time::Instant::now() >= deadline {
                                break;
                            }
                            let mut group = track_producer.append_group();
                            let frame = gen.generate_with_timestamp();
                            let frame_len = frame.len() as u64;
                            group.write_frame(frame);
                            group.close();

                            pm.groups_total.fetch_add(1, Ordering::Relaxed);
                            pm.frames_total.fetch_add(1, Ordering::Relaxed);
                            pm.bytes_total.fetch_add(frame_len, Ordering::Relaxed);

                            tokio::time::sleep(interval).await;
                        }
                    }));
                }

                for h in handles {
                    let _ = h.await;
                }
                Ok(())
            }
            .await;
            if let Err(e) = result {
                debug!("probe publisher error: {e}");
            }
        });

        // --- Subscriber task (uses SubscriberWorker with latency measurement) ---
        let sub_config = SubscribeConfig {
            relay: url.clone(),
            broadcast: self.config.broadcast.clone(),
            tracks: self.config.tracks,
            duration_secs: self.config.duration_secs,
            validate: false,
            insecure: self.config.insecure,
            output: OutputFormat::Text,
            metrics_interval_secs: self.config.metrics_interval_secs,
            frame_size: frame_size,
            static_dir: None,
            output_dir: None,
        };

        let sub_worker = SubscriberWorker::new(sub_config, Arc::clone(&self.sub_metrics))
            .with_latency(Arc::clone(&self.latency));

        let sub_url = url.clone();
        let sub_handle = tokio::spawn(async move {
            if let Err(e) = sub_worker.run(sub_url).await {
                debug!("probe subscriber error: {e}");
            }
        });

        let _ = pub_handle.await;
        let _ = sub_handle.await;

        Ok(())
    }
}
