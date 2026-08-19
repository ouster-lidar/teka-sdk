//! Parse `tegrastats` output and fan out samples to WebSocket clients.

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
    pub ram_used_mb: u32,
    pub ram_total_mb: u32,
    pub ram_pct: f32,
    pub cpu_pct: f32,
    pub gpu_pct: f32,
    pub gpu_temp_c: f32,
    pub power_mw: u32,
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

async fn tegra_supervisor(tx: broadcast::Sender<TegraSample>, last: Arc<RwLock<Option<TegraSample>>>) {
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
        if let Some(sample) = parse_line(line) {
            *last.write() = Some(sample.clone());
            let _ = tx.send(sample);
        }
    }

    let status = child.wait().await?;
    anyhow::bail!("tegrastats exited with {status}");
}

pub fn parse_line(line: &str) -> Option<TegraSample> {
    let ram = parse_ram(line)?;
    let cpu_pct = parse_cpu_avg(line)?;
    let gpu_pct = parse_gpu_pct(line)?;
    let gpu_temp_c = parse_gpu_temp(line)?;
    let power_mw = parse_power_mw(line)?;
    let ram_pct = if ram.1 > 0 {
        (ram.0 as f32 / ram.1 as f32) * 100.0
    } else {
        0.0
    };
    Some(TegraSample {
        ts_ms: now_ms(),
        raw: line.to_string(),
        ram_used_mb: ram.0,
        ram_total_mb: ram.1,
        ram_pct,
        cpu_pct,
        gpu_pct,
        gpu_temp_c,
        power_mw,
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn parse_ram(line: &str) -> Option<(u32, u32)> {
    let idx = line.find("RAM ")?;
    let rest = &line[idx + 4..];
    let end = rest.find("MB")?;
    let pair = &rest[..end];
    let (used, total) = pair.split_once('/')?;
    Some((used.parse().ok()?, total.parse().ok()?))
}

fn parse_cpu_avg(line: &str) -> Option<f32> {
    let start = line.find("CPU [")? + 5;
    let end = line[start..].find(']')? + start;
    let cores = &line[start..end];
    let mut sum = 0.0f32;
    let mut count = 0u32;
    for part in cores.split(',') {
        let part = part.trim();
        if part == "off" {
            continue;
        }
        let pct_str = part.split('%').next()?;
        let pct: f32 = pct_str.parse().ok()?;
        sum += pct;
        count += 1;
    }
    if count == 0 {
        return Some(0.0);
    }
    Some(sum / count as f32)
}

fn parse_gpu_pct(line: &str) -> Option<f32> {
    let idx = line.find("GR3D_FREQ ")? + 10;
    let rest = &line[idx..];
    let end = rest.find('%')?;
    rest[..end].parse().ok()
}

fn parse_gpu_temp(line: &str) -> Option<f32> {
    let idx = line.find("gpu@")? + 4;
    let rest = &line[idx..];
    let end = rest.find('C')?;
    rest[..end].parse().ok()
}

fn parse_power_mw(line: &str) -> Option<u32> {
    let idx = line.find("VDD_IN ")? + 7;
    let rest = &line[idx..];
    let end = rest.find('m')?;
    rest[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "08-18-2026 18:57:41 RAM 1825/7542MB (lfb 25x4MB) CPU [5%@729,7%@729,6%@729,7%@729,off,off] GR3D_FREQ 0% cv0@44.281C/44.281C cpu@45.281C/45.281C soc2@44.593C/44.593C soc0@44.625C/44.625C cv1@44.343C/44.343C gpu@44.093C/44.093C tj@46.375C/46.375C soc1@46.375C/46.375C cv2@44.375C/44.375C VDD_IN 5954mW/5954mW/5954mW VDD_CPU_GPU_CV 573mW/573mW/573mW VDD_SOC 1721mW/1721mW/1721mW";

    #[test]
    fn parses_tegrastats_line() {
        let s = parse_line(SAMPLE).expect("parse");
        assert_eq!(s.ram_used_mb, 1825);
        assert_eq!(s.ram_total_mb, 7542);
        assert!((s.ram_pct - 24.2).abs() < 0.2);
        assert!((s.cpu_pct - 6.25).abs() < 0.01);
        assert_eq!(s.gpu_pct, 0.0);
        assert!((s.gpu_temp_c - 44.093).abs() < 0.001);
        assert_eq!(s.power_mw, 5954);
    }
}
