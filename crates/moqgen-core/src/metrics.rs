use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use hdrhistogram::Histogram;

// ---------------------------------------------------------------------------
// Publish metrics
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct PublishMetrics {
    pub groups_total: AtomicU64,
    pub frames_total: AtomicU64,
    pub bytes_total: AtomicU64,
    pub errors_total: AtomicU64,
}

impl PublishMetrics {
    pub fn snapshot(&self) -> PublishSnapshot {
        PublishSnapshot {
            groups_total: self.groups_total.load(Ordering::Relaxed),
            frames_total: self.frames_total.load(Ordering::Relaxed),
            bytes_total: self.bytes_total.load(Ordering::Relaxed),
            errors_total: self.errors_total.load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PublishSnapshot {
    pub groups_total: u64,
    pub frames_total: u64,
    pub bytes_total: u64,
    pub errors_total: u64,
}

// ---------------------------------------------------------------------------
// Subscribe metrics
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct SubscribeMetrics {
    pub groups_total: AtomicU64,
    pub frames_total: AtomicU64,
    pub bytes_total: AtomicU64,
    pub errors_total: AtomicU64,
    pub groups_dropped: AtomicU64,
}

impl SubscribeMetrics {
    pub fn snapshot(&self) -> SubscribeSnapshot {
        SubscribeSnapshot {
            groups_total: self.groups_total.load(Ordering::Relaxed),
            frames_total: self.frames_total.load(Ordering::Relaxed),
            bytes_total: self.bytes_total.load(Ordering::Relaxed),
            errors_total: self.errors_total.load(Ordering::Relaxed),
            groups_dropped: self.groups_dropped.load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SubscribeSnapshot {
    pub groups_total: u64,
    pub frames_total: u64,
    pub bytes_total: u64,
    pub errors_total: u64,
    pub groups_dropped: u64,
}

// ---------------------------------------------------------------------------
// Latency histogram (microseconds)
// ---------------------------------------------------------------------------

pub struct LatencyHistogram {
    inner: Mutex<Histogram<u64>>,
}

impl LatencyHistogram {
    pub fn new() -> anyhow::Result<Self> {
        let hist = Histogram::<u64>::new(3).map_err(|e| anyhow::anyhow!("histogram init: {e}"))?;
        Ok(Self {
            inner: Mutex::new(hist),
        })
    }

    pub fn record(&self, us: u64) {
        if let Ok(mut h) = self.inner.lock() {
            // saturating record: ignore out-of-range values
            let _ = h.record(us);
        }
    }

    pub fn snapshot(&self) -> Option<LatencySnapshot> {
        let h = self.inner.lock().ok()?;
        if h.len() == 0 {
            return None;
        }
        Some(LatencySnapshot {
            count: h.len(),
            min_us: h.min(),
            max_us: h.max(),
            mean_us: h.mean() as u64,
            p50_us: h.value_at_percentile(50.0),
            p90_us: h.value_at_percentile(90.0),
            p99_us: h.value_at_percentile(99.0),
        })
    }
}

#[derive(Clone, Debug)]
pub struct LatencySnapshot {
    pub count: u64,
    pub min_us: u64,
    pub max_us: u64,
    pub mean_us: u64,
    pub p50_us: u64,
    pub p90_us: u64,
    pub p99_us: u64,
}
