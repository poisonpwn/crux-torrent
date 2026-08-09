use std::time::Instant;

use super::progress::PieceDownloadProgress;

/// paces how many blocks a peer connection is allowed to have outstanding at once.
#[derive(Debug)]
pub(super) struct RequestWindow {
    ewma_bytes_per_sec: f64,
    last_arrival: Option<Instant>,
}

impl RequestWindow {
    const MIN_BLOCKS: u32 = 5;
    // upper bound on outstanding blocks, purely to cap worst-case memory use against a very
    // fast/high-latency peer; 500 blocks is ~8MiB.
    const MAX_BLOCKS: u32 = 500;
    const TARGET_QUEUE_SECONDS: f64 = 2.0;
    // weight given to each new sample for the exponential averaging
    const EWMA_ALPHA: f64 = 0.2;

    pub fn new() -> Self {
        Self {
            ewma_bytes_per_sec: 0.0,
            last_arrival: None,
        }
    }

    pub fn on_block_received(&mut self, len: u32) {
        let now = Instant::now();
        if let Some(last) = self.last_arrival {
            let elapsed = now.duration_since(last).as_secs_f64().max(0.001);
            let instantaneous = len as f64 / elapsed;
            self.ewma_bytes_per_sec = if self.ewma_bytes_per_sec == 0.0 {
                instantaneous
            } else {
                Self::EWMA_ALPHA * instantaneous
                    + (1.0 - Self::EWMA_ALPHA) * self.ewma_bytes_per_sec
            };
        }
        self.last_arrival = Some(now);
    }

    pub fn desired_pending_blocks(&self) -> u32 {
        let desired_bytes = self.ewma_bytes_per_sec * Self::TARGET_QUEUE_SECONDS;
        let desired_blocks =
            (desired_bytes / PieceDownloadProgress::MAX_BLOCK_SIZE as f64).ceil() as u32;
        desired_blocks.clamp(Self::MIN_BLOCKS, Self::MAX_BLOCKS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_the_minimum_before_any_measurement() {
        let window = RequestWindow::new();
        assert_eq!(window.desired_pending_blocks(), RequestWindow::MIN_BLOCKS);
    }

    #[test]
    fn a_single_sample_has_no_elapsed_time_to_measure_against() {
        let mut window = RequestWindow::new();
        window.on_block_received(16384);
        // first arrival only seeds `last_arrival`; there's no prior timestamp to compute a rate
        // from yet, so the window should still be at its floor.
        assert_eq!(window.desired_pending_blocks(), RequestWindow::MIN_BLOCKS);
    }

    #[test]
    fn ramps_up_past_the_minimum_for_a_sustained_fast_peer() {
        let mut window = RequestWindow::new();
        // back-to-back arrivals with negligible elapsed time between them read as a very high
        // instantaneous rate (clamped to the 1ms floor), which should quickly pull the EWMA
        // estimate, and so the desired window, up past the starting minimum.
        for _ in 0..50 {
            window.on_block_received(16384);
        }
        assert!(window.desired_pending_blocks() > RequestWindow::MIN_BLOCKS);
    }

    #[test]
    fn never_exceeds_the_configured_cap() {
        let mut window = RequestWindow::new();
        window.ewma_bytes_per_sec = 1_000_000_000.0; // absurdly fast, to probe the ceiling
        assert_eq!(window.desired_pending_blocks(), RequestWindow::MAX_BLOCKS);
    }
}
