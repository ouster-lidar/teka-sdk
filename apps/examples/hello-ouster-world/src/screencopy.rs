//! Browser JPEG capture.
//!
//! wlroots on NVIDIA Tegra rejects `zwlr_screencopy` **wl_shm** `copy` (`Failed`).
//! `grim` uses linux-dmabuf and works; run it from this process next to the WS
//! publisher (not from Python / PointViz).

use std::process::Command;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Context;
use image::codecs::jpeg::JpegEncoder;
use image::{ColorType, DynamicImage};

use crate::display::{stream_dims, write_stream_target, DisplayState};

const JPEG_QUALITY: u8 = 40;

pub fn spawn_capture(display: DisplayState) {
    thread::Builder::new()
        .name("hello-ouster-world-screencopy".into())
        .spawn(move || {
            if let Err(e) = run_capture_loop(display) {
                tracing::warn!(?e, "screencopy thread exited");
            }
        })
        .expect("spawn screencopy thread");
}

fn apply_wayland_env() {
    let xdg = std::env::var("HELLO_OUSTER_WORLD_XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| "/run/hello-ouster-world".into());
    let wd = std::env::var("HELLO_OUSTER_WORLD_WAYLAND_DISPLAY").unwrap_or_else(|_| {
        for name in ["wayland-1", "wayland-0"] {
            if std::path::Path::new(&xdg).join(name).exists() {
                return name.to_string();
            }
        }
        "wayland-1".into()
    });
    std::env::set_var("XDG_RUNTIME_DIR", &xdg);
    std::env::set_var("WAYLAND_DISPLAY", &wd);
}

fn grim_bin() -> String {
    std::env::var("HELLO_OUSTER_WORLD_GRIM").unwrap_or_else(|_| "grim".into())
}

fn capture_jpeg(preferred: Option<&str>) -> anyhow::Result<Vec<u8>> {
    apply_wayland_env();
    let mut cmd = Command::new(grim_bin());
    cmd.args(["-t", "jpeg", "-q", &JPEG_QUALITY.to_string()]);
    if let Some(output) = preferred {
        cmd.args(["-o", output]);
    }
    cmd.arg("-");
    let out = cmd.output().context("spawn grim")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("grim failed: {err}");
    }
    if out.stdout.len() < 3 || out.stdout[0] != 0xff || out.stdout[1] != 0xd8 {
        anyhow::bail!("grim did not write a JPEG ({} bytes)", out.stdout.len());
    }
    Ok(out.stdout)
}

fn maybe_resize_jpeg(jpeg: Vec<u8>, out_w: u32, out_h: u32) -> anyhow::Result<Vec<u8>> {
    let img = image::load_from_memory(&jpeg).context("decode grim jpeg")?;
    if img.width() == out_w && img.height() == out_h {
        return Ok(jpeg);
    }
    let resized: DynamicImage =
        DynamicImage::ImageRgb8(image::imageops::resize(
            &img.to_rgb8(),
            out_w,
            out_h,
            image::imageops::FilterType::Triangle,
        ));
    let rgb = resized.to_rgb8();
    let mut buf = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut buf, JPEG_QUALITY);
    enc.encode(rgb.as_raw(), rgb.width(), rgb.height(), ColorType::Rgb8.into())
        .context("jpeg encode")?;
    Ok(buf)
}

fn run_capture_loop(display: DisplayState) -> anyhow::Result<()> {
    let gate = display.inner.capture_gate.subscribe();
    let mut last_err_log = Instant::now() - Duration::from_secs(60);
    let mut next_frame = Instant::now();
    let mut logged_ready = false;

    loop {
        if !*gate.borrow() {
            logged_ready = false;
            let _ = write_stream_target(&display);
            thread::sleep(Duration::from_millis(200));
            continue;
        }
        let _ = write_stream_target(&display);

        if !logged_ready {
            tracing::info!(grim = %grim_bin(), "screencopy using grim (dmabuf)");
            logged_ready = true;
        }

        let fps = display.max_framerate().max(1);
        let interval = Duration::from_millis(1000 / fps);
        let now = Instant::now();
        if now < next_frame {
            thread::sleep(next_frame - now);
            continue;
        }
        next_frame = Instant::now() + interval;

        let preferred = display.inner.viz_output.lock().clone();
        let started = Instant::now();
        match capture_jpeg(preferred.as_deref()) {
            Ok(jpeg) => {
                let res = *display.inner.resolution.lock();
                let (src_w, src_h) = *display.inner.source_size.lock();
                let (dw, dh) = stream_dims(res, src_w, src_h);
                *display.inner.scaled_size.lock() = (dw, dh);
                match maybe_resize_jpeg(jpeg, dw, dh) {
                    Ok(jpeg) => {
                        display.inner.last_capture_ms.store(
                            started.elapsed().as_millis() as u64,
                            Ordering::Relaxed,
                        );
                        display.set_error(None);
                        let _ = display.inner.frame_tx.send(Arc::new(jpeg));
                    }
                    Err(e) => {
                        if last_err_log.elapsed() >= Duration::from_secs(10) {
                            tracing::warn!(?e, "screencopy jpeg resize failed");
                            last_err_log = Instant::now();
                        }
                        display.set_error(Some(e.to_string()));
                    }
                }
            }
            Err(e) => {
                if last_err_log.elapsed() >= Duration::from_secs(10) {
                    tracing::warn!(?e, "screencopy grim capture failed");
                    last_err_log = Instant::now();
                }
                display.set_error(Some(e.to_string()));
            }
        }
    }
}
