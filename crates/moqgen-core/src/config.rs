use std::path::PathBuf;

use url::Url;

#[derive(Clone, Debug)]
pub struct PublishConfig {
    pub relay: Url,
    pub broadcast: String,
    pub tracks: usize,
    pub rate: f64,
    pub group_size: usize,
    pub frame_size: usize,
    pub duration_secs: u64,
    pub workers: usize,
    pub insecure: bool,
    pub payload_type: PayloadType,
    pub output: OutputFormat,
    pub metrics_interval_secs: u64,
    /// Serve files from this directory; each file becomes one track.
    /// When set, overrides `tracks` and uses filenames as track names.
    pub static_dir: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct SubscribeConfig {
    pub relay: Url,
    pub broadcast: String,
    /// Number of tracks to subscribe to (track-0 .. track-N-1)
    pub tracks: usize,
    pub duration_secs: u64,
    pub validate: bool,
    pub insecure: bool,
    pub output: OutputFormat,
    pub metrics_interval_secs: u64,
    /// Expected frame size for validation (0 = skip size check)
    pub frame_size: usize,
    /// Discover track names from filenames in this directory.
    pub static_dir: Option<PathBuf>,
    /// Write each received track to a file in this directory.
    pub output_dir: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct ProbeConfig {
    pub relay: Url,
    pub broadcast: String,
    pub tracks: usize,
    pub rate: f64,
    pub frame_size: usize,
    pub duration_secs: u64,
    pub insecure: bool,
    pub output: OutputFormat,
    pub metrics_interval_secs: u64,
}

#[derive(Clone, Debug, Default)]
pub enum PayloadType {
    #[default]
    Random,
    Sequential,
    Fixed(u8),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}
