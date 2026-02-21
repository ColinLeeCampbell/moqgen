use moqgen_core::metrics::{LatencySnapshot, PublishSnapshot, SubscribeSnapshot};
use serde::Serialize;

// ---------------------------------------------------------------------------
// Serialisable snapshot types (for JSON output)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct PublishReport {
    pub elapsed_secs: f64,
    pub groups_total: u64,
    pub frames_total: u64,
    pub bytes_total: u64,
    pub errors_total: u64,
    pub groups_per_sec: f64,
    pub frames_per_sec: f64,
    pub bytes_per_sec: f64,
    pub mbps: f64,
}

impl PublishReport {
    pub fn from_snapshot(snap: &PublishSnapshot, elapsed_secs: f64) -> Self {
        let rate = |n: u64| if elapsed_secs > 0.0 { n as f64 / elapsed_secs } else { 0.0 };
        Self {
            elapsed_secs,
            groups_total: snap.groups_total,
            frames_total: snap.frames_total,
            bytes_total: snap.bytes_total,
            errors_total: snap.errors_total,
            groups_per_sec: rate(snap.groups_total),
            frames_per_sec: rate(snap.frames_total),
            bytes_per_sec: rate(snap.bytes_total),
            mbps: rate(snap.bytes_total) * 8.0 / 1_000_000.0,
        }
    }
}

#[derive(Serialize)]
pub struct SubscribeReport {
    pub elapsed_secs: f64,
    pub groups_total: u64,
    pub frames_total: u64,
    pub bytes_total: u64,
    pub errors_total: u64,
    pub groups_dropped: u64,
    pub groups_per_sec: f64,
    pub frames_per_sec: f64,
    pub bytes_per_sec: f64,
    pub mbps: f64,
}

impl SubscribeReport {
    pub fn from_snapshot(snap: &SubscribeSnapshot, elapsed_secs: f64) -> Self {
        let rate = |n: u64| if elapsed_secs > 0.0 { n as f64 / elapsed_secs } else { 0.0 };
        Self {
            elapsed_secs,
            groups_total: snap.groups_total,
            frames_total: snap.frames_total,
            bytes_total: snap.bytes_total,
            errors_total: snap.errors_total,
            groups_dropped: snap.groups_dropped,
            groups_per_sec: rate(snap.groups_total),
            frames_per_sec: rate(snap.frames_total),
            bytes_per_sec: rate(snap.bytes_total),
            mbps: rate(snap.bytes_total) * 8.0 / 1_000_000.0,
        }
    }
}

#[derive(Serialize)]
pub struct ProbeReport {
    pub elapsed_secs: f64,
    pub publish: PublishReport,
    pub subscribe: SubscribeReport,
    pub latency_count: u64,
    pub latency_min_us: u64,
    pub latency_max_us: u64,
    pub latency_mean_us: u64,
    pub latency_p50_us: u64,
    pub latency_p90_us: u64,
    pub latency_p99_us: u64,
}

impl ProbeReport {
    pub fn new(
        pub_report: PublishReport,
        sub_report: SubscribeReport,
        lat: &LatencySnapshot,
        elapsed_secs: f64,
    ) -> Self {
        Self {
            elapsed_secs,
            publish: pub_report,
            subscribe: sub_report,
            latency_count: lat.count,
            latency_min_us: lat.min_us,
            latency_max_us: lat.max_us,
            latency_mean_us: lat.mean_us,
            latency_p50_us: lat.p50_us,
            latency_p90_us: lat.p90_us,
            latency_p99_us: lat.p99_us,
        }
    }
}

// ---------------------------------------------------------------------------
// Text formatting
// ---------------------------------------------------------------------------

pub fn print_publish_text(report: &PublishReport) {
    println!(
        "publish | groups: {}/s ({} total)  frames: {}/s  throughput: {:.2} Mbps  errors: {}",
        report.groups_per_sec as u64,
        report.groups_total,
        report.frames_per_sec as u64,
        report.mbps,
        report.errors_total,
    );
}

pub fn print_subscribe_text(report: &SubscribeReport) {
    println!(
        "subscribe | groups: {}/s ({} total)  frames: {}/s  throughput: {:.2} Mbps  dropped: {}  errors: {}",
        report.groups_per_sec as u64,
        report.groups_total,
        report.frames_per_sec as u64,
        report.mbps,
        report.groups_dropped,
        report.errors_total,
    );
}

pub fn print_latency_text(report: &ProbeReport) {
    if report.latency_count == 0 {
        println!("latency | no samples yet");
        return;
    }
    println!(
        "latency | n={} min={}µs p50={}µs p90={}µs p99={}µs max={}µs mean={}µs",
        report.latency_count,
        report.latency_min_us,
        report.latency_p50_us,
        report.latency_p90_us,
        report.latency_p99_us,
        report.latency_max_us,
        report.latency_mean_us,
    );
}

pub fn print_json<T: Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("JSON serialization error: {e}"),
    }
}
