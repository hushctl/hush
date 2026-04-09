use std::time::Duration;

use sysinfo::System;
use tokio::sync::broadcast;
use tokio::time::{interval, MissedTickBehavior};
use tracing::info;

use crate::protocol::ServerMessage;

const MEMORY_POLL_INTERVAL: Duration = Duration::from_secs(15);
const WARNING_THRESHOLD: f64 = 0.25; // <25% available → warning
const CRITICAL_THRESHOLD: f64 = 0.10; // <10% available → critical

#[derive(Debug, Clone, Copy, PartialEq)]
enum PressureLevel {
    Normal,
    Warning,
    Critical,
}

impl PressureLevel {
    fn as_str(self) -> &'static str {
        match self {
            PressureLevel::Normal => "normal",
            PressureLevel::Warning => "warning",
            PressureLevel::Critical => "critical",
        }
    }
}

fn classify(ratio: f64) -> PressureLevel {
    if ratio < CRITICAL_THRESHOLD {
        PressureLevel::Critical
    } else if ratio < WARNING_THRESHOLD {
        PressureLevel::Warning
    } else {
        PressureLevel::Normal
    }
}

pub fn spawn(machine_id: String, tx: broadcast::Sender<ServerMessage>) {
    tokio::spawn(async move {
        let mut sys = System::new();
        let mut last_level = PressureLevel::Normal;
        let mut ticker = interval(MEMORY_POLL_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            sys.refresh_memory();
            let avail = sys.available_memory();
            let total = sys.total_memory();
            if total == 0 {
                continue;
            }
            let ratio = avail as f64 / total as f64;
            let level = classify(ratio);
            if level != last_level {
                last_level = level;
                let _ = tx.send(ServerMessage::MemoryPressure {
                    machine_id: machine_id.clone(),
                    level: level.as_str().to_string(),
                    available_bytes: avail,
                    total_bytes: total,
                });
            }
        }
    });
    info!(
        "spawned memory monitor (interval={}s, warn=<{}%, crit=<{}%)",
        MEMORY_POLL_INTERVAL.as_secs(),
        (WARNING_THRESHOLD * 100.0) as u32,
        (CRITICAL_THRESHOLD * 100.0) as u32,
    );
}
