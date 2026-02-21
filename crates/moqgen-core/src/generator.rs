use bytes::Bytes;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::PayloadType;

pub struct FrameGenerator {
    size: usize,
    payload_type: PayloadType,
    counter: u64,
}

impl FrameGenerator {
    pub fn new(size: usize, payload_type: PayloadType) -> Self {
        Self {
            size,
            payload_type,
            counter: 0,
        }
    }

    pub fn generate(&mut self) -> Bytes {
        let mut buf = vec![0u8; self.size];
        match &self.payload_type {
            PayloadType::Random => {
                // Cheap pseudo-random fill using counter + position
                for (i, b) in buf.iter_mut().enumerate() {
                    *b = (self.counter.wrapping_add(i as u64) & 0xFF) as u8;
                }
            }
            PayloadType::Sequential => {
                let count_bytes = self.counter.to_be_bytes();
                let len = count_bytes.len().min(buf.len());
                buf[..len].copy_from_slice(&count_bytes[..len]);
            }
            PayloadType::Fixed(byte) => {
                let fill = *byte;
                buf.fill(fill);
            }
        }
        self.counter += 1;
        Bytes::from(buf)
    }

    /// Generate a frame with a u64 microsecond timestamp in the first 8 bytes.
    /// `size` must be at least 8; smaller sizes are padded to 8.
    pub fn generate_with_timestamp(&mut self) -> Bytes {
        let actual_size = self.size.max(8);
        let mut buf = vec![0u8; actual_size];

        let now_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;

        buf[..8].copy_from_slice(&now_us.to_be_bytes());

        // Fill the rest with the counter byte for easy debugging
        for b in buf[8..].iter_mut() {
            *b = (self.counter & 0xFF) as u8;
        }

        self.counter += 1;
        Bytes::from(buf)
    }
}

/// Extract the microsecond timestamp written by `generate_with_timestamp`.
/// Returns `None` if the frame is shorter than 8 bytes.
pub fn read_timestamp_us(frame: &[u8]) -> Option<u64> {
    if frame.len() < 8 {
        return None;
    }
    let ts = u64::from_be_bytes(frame[..8].try_into().ok()?);
    Some(ts)
}
