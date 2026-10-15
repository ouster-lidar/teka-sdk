//! Fan out raw `tegrastats` lines to WebSocket clients. Parsing lives in the web UI.

use std::process::Stdio;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::broadcast;
use tracing::{info, warn};

const INTERVAL_MS: &str = "1000";
const CHANNEL_CAP: usize = 64;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TegraSample {
    pub ts_ms: u64,
    pub raw: String,
}

#[derive(Clone)]
pub struct TegraMonitor {
    pub tx: broadcast::Sender<TegraSample>,
    pub last: Arc<RwLock<Option<TegraSample>>>,
}

impl TegraMonitor {
    pub fn spawn() -> Self {
        let (tx, _) = broadcast::channel(CHANNEL_CAP);
        let monitor = Self {
            tx: tx.clone(),
            last: Arc::new(RwLock::new(None)),
        };
        let last = monitor.last.clone();
        tokio::spawn(async move {
            tegra_supervisor(tx, last).await;
        });
        monitor
    }

    pub fn publish(&self, sample: TegraSample) {
        *self.last.write() = Some(sample.clone());
        let _ = self.tx.send(sample);
    }
}

async fn tegra_supervisor(
    tx: broadcast::Sender<TegraSample>,
    last: Arc<RwLock<Option<TegraSample>>>,
) {
    loop {
        match run_tegrastats(&tx, &last).await {
            Ok(()) => warn!("tegrastats exited; restarting in 2s"),
            Err(e) => warn!("tegrastats error: {e}; restarting in 2s"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

async fn run_tegrastats(
    tx: &broadcast::Sender<TegraSample>,
    last: &Arc<RwLock<Option<TegraSample>>>,
) -> anyhow::Result<()> {
    let mut child = Command::new("tegrastats")
        .args(["--interval", INTERVAL_MS])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("tegrastats stdout not piped"))?;

    info!("tegrastats started (interval {INTERVAL_MS}ms)");
    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let sample = TegraSample {
            ts_ms: now_ms(),
            raw: line.to_string(),
        };
        *last.write() = Some(sample.clone());
        let _ = tx.send(sample);
    }

    let status = child.wait().await?;
    anyhow::bail!("tegrastats exited with {status}");
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
