//! Browser JPEG capture: zwlr_screencopy wl_shm → nvvidconv/nvjpegenc → WS.

use std::fs::{File, OpenOptions};
use std::os::fd::{AsFd, AsRawFd};
use std::path::PathBuf;
use std::ptr;
use std::slice;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Context;
use drm_fourcc::DrmFourcc;
use gstreamer::glib;
use gstreamer::prelude::*;
use gstreamer::{Caps, FlowError, FlowSuccess, Fraction, Pipeline, State};
use gstreamer_app::{AppSink, AppSinkCallbacks, AppSrc};
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_buffer, wl_output, wl_registry, wl_shm, wl_shm_pool};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1, zwlr_screencopy_manager_v1,
};

use crate::display::{write_stream_target, DisplayState};

const DRM_FORMAT_XRGB8888: u32 = DrmFourcc::Xrgb8888 as u32;

const PROT_READ: i32 = 1;
const MAP_SHARED: i32 = 1;

extern "C" {
    fn mmap(
        addr: *mut std::ffi::c_void,
        len: usize,
        prot: i32,
        flags: i32,
        fd: i32,
        offset: i64,
    ) -> *mut std::ffi::c_void;
    fn munmap(addr: *mut std::ffi::c_void, len: usize) -> i32;
}

struct MappedShm {
    ptr: *mut u8,
    len: usize,
}

unsafe impl Send for MappedShm {}

impl MappedShm {
    fn map(file: &File, len: usize) -> anyhow::Result<Self> {
        if len == 0 {
            anyhow::bail!("mmap size 0");
        }
        let ptr = unsafe {
            mmap(
                ptr::null_mut(),
                len,
                PROT_READ,
                MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if ptr.is_null() || ptr == usize::MAX as *mut std::ffi::c_void {
            anyhow::bail!("mmap screencopy shm failed");
        }
        Ok(Self {
            ptr: ptr.cast(),
            len,
        })
    }

    fn bytes(&self, size: usize) -> anyhow::Result<&[u8]> {
        if size > self.len {
            anyhow::bail!("mmap slice {size} > mapping {}", self.len);
        }
        Ok(unsafe { slice::from_raw_parts(self.ptr, size) })
    }
}

impl Drop for MappedShm {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                let _ = munmap(self.ptr.cast(), self.len);
            }
            self.ptr = ptr::null_mut();
        }
    }
}

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

pub(crate) fn apply_wayland_env() {
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

fn gst_format_for_drm(fourcc: DrmFourcc) -> Option<&'static str> {
    match fourcc {
        DrmFourcc::Xrgb8888 | DrmFourcc::Argb8888 => Some("BGRx"),
        DrmFourcc::Xbgr8888 | DrmFourcc::Abgr8888 => Some("RGBx"),
        DrmFourcc::Bgr888 => Some("BGR"),
        DrmFourcc::Rgb888 => Some("RGB"),
        _ => None,
    }
}

fn drm_fourcc_for_wl_shm(format: u32) -> Option<DrmFourcc> {
    // wl_shm.format: 0=argb8888, 1=xrgb8888, else fourcc.
    match format {
        0 => Some(DrmFourcc::Argb8888),
        1 => Some(DrmFourcc::Xrgb8888),
        x if x == u32::from_le_bytes(*b"XR24") => Some(DrmFourcc::Xrgb8888),
        x if x == u32::from_le_bytes(*b"AR24") => Some(DrmFourcc::Argb8888),
        x if x == u32::from_le_bytes(*b"XB24") => Some(DrmFourcc::Xbgr8888),
        x if x == u32::from_le_bytes(*b"AB24") => Some(DrmFourcc::Abgr8888),
        _ => DrmFourcc::try_from(format).ok(),
    }
}

fn fourcc_u32(fourcc: DrmFourcc) -> u32 {
    fourcc as u32
}

fn pack_to_bgrx(
    src: &[u8],
    src_stride: u32,
    width: u32,
    height: u32,
    src_bpp: usize,
    swap_rb: bool,
) -> anyhow::Result<Vec<u8>> {
    let row = (width as usize).saturating_mul(src_bpp);
    let stride = src_stride as usize;
    let need = stride.saturating_mul(height as usize);
    if src.len() < need {
        anyhow::bail!(
            "mapped buffer too small: {} < stride*height {}",
            src.len(),
            need
        );
    }
    if stride < row {
        anyhow::bail!("stride {stride} < row bytes {row}");
    }
    let dst_row = (width as usize).saturating_mul(4);
    if src_bpp == 4 && !swap_rb && stride == row {
        let total = dst_row.saturating_mul(height as usize);
        return Ok(src[..total].to_vec());
    }
    let mut out = vec![0u8; dst_row.saturating_mul(height as usize)];
    for y in 0..height as usize {
        let s = y * stride;
        let d = y * dst_row;
        if src_bpp == 4 {
            out[d..d + dst_row].copy_from_slice(&src[s..s + dst_row]);
            if swap_rb {
                for x in 0..width as usize {
                    let di = d + x * 4;
                    out.swap(di, di + 2);
                }
            }
        } else if src_bpp == 3 {
            for x in 0..width as usize {
                let si = s + x * 3;
                let di = d + x * 4;
                if swap_rb {
                    out[di] = src[si + 2];
                    out[di + 1] = src[si + 1];
                    out[di + 2] = src[si];
                } else {
                    out[di] = src[si];
                    out[di + 1] = src[si + 1];
                    out[di + 2] = src[si + 2];
                }
                out[di + 3] = 0xff;
            }
        } else {
            anyhow::bail!("unsupported src bpp {src_bpp}");
        }
    }
    Ok(out)
}

fn wl_shm_format_for_drm(fourcc: DrmFourcc) -> Option<wl_shm::Format> {
    match fourcc {
        DrmFourcc::Xrgb8888 => Some(wl_shm::Format::Xrgb8888),
        DrmFourcc::Argb8888 => Some(wl_shm::Format::Argb8888),
        DrmFourcc::Xbgr8888 => Some(wl_shm::Format::Xbgr8888),
        DrmFourcc::Abgr8888 => Some(wl_shm::Format::Abgr8888),
        DrmFourcc::Bgr888 => Some(wl_shm::Format::Bgr888),
        DrmFourcc::Rgb888 => Some(wl_shm::Format::Rgb888),
        _ => None,
    }
}

fn set_element_prop_if_exists(
    element: &gstreamer::Element,
    name: &str,
    value: impl Into<glib::Value> + Clone,
) {
    let Some(spec) = element.find_property(name) else {
        tracing::debug!(element = %element.name(), %name, "gstreamer property missing");
        return;
    };
    let v: glib::Value = value.into();
    if spec.value_type() != v.type_() {
        tracing::debug!(
            element = %element.name(),
            %name,
            expected = ?spec.value_type(),
            got = ?v.type_(),
            "gstreamer property type mismatch"
        );
        return;
    }
    element.set_property_from_value(name, &v);
}

struct EncodePipeline {
    pipeline: Pipeline,
    appsrc: AppSrc,
    width: u32,
    height: u32,
    fps: u64,
    drm_fourcc: DrmFourcc,
}

impl EncodePipeline {
    fn new(
        display: &DisplayState,
        width: u32,
        height: u32,
        fps: u64,
        drm_fourcc: DrmFourcc,
    ) -> anyhow::Result<Self> {
        let gst_format = gst_format_for_drm(drm_fourcc)
            .ok_or_else(|| anyhow::anyhow!("unsupported screencopy drm format {drm_fourcc:?}"))?;

        let pipeline = Pipeline::new();
        let appsrc = gstreamer::ElementFactory::make("appsrc")
            .name("screencopy_src")
            .build()?
            .downcast::<AppSrc>()
            .unwrap();
        tracing::debug!("gst: creating nvvidconv");
        let nvvidconv = gstreamer::ElementFactory::make("nvvidconv")
            .name("screencopy_nvvidconv")
            .build()
            .context("nvvidconv missing (Jetson L4T GStreamer plugins required)")?;
        tracing::debug!("gst: creating jpeg encoder");
        let encoder = gstreamer::ElementFactory::make("nvjpegenc")
            .name("screencopy_jpegenc")
            .build()
            .or_else(|_| {
                tracing::warn!("nvjpegenc missing; falling back to jpegenc");
                gstreamer::ElementFactory::make("jpegenc")
                    .name("screencopy_jpegenc")
                    .build()
            })
            .context("nvjpegenc/jpegenc missing")?;
        tracing::debug!("gst: creating capsfilter/appsink");
        let nv12 = gstreamer::ElementFactory::make("capsfilter")
            .name("screencopy_nv12")
            .build()?;
        let appsink = gstreamer::ElementFactory::make("appsink")
            .name("screencopy_sink")
            .build()?
            .downcast::<AppSink>()
            .unwrap();
        nv12.set_property(
            "caps",
            Caps::builder("video/x-raw")
                .features(["memory:NVMM"])
                .field("format", "NV12")
                // Full-range BT.601 (JFIF); limited range shows black as 16 in browsers.
                .field("colorimetry", "1:4:0:0")
                .build(),
        );
        tracing::debug!("gst: encoder props (jpeg)");

        let even_w = width - (width % 2);
        let even_h = height - (height % 2);
        if width != even_w {
            set_element_prop_if_exists(&nvvidconv, "right", 1u32);
        }
        if height != even_h {
            set_element_prop_if_exists(&nvvidconv, "bottom", 1u32);
        }

        set_element_prop_if_exists(&encoder, "quality", 80i32);
        if nvvidconv.find_property("compute-hw").is_some() {
            // GPU conversion intermittently produces green JPEG frames on Orin.
            nvvidconv.set_property_from_str("compute-hw", "VIC");
        }

        tracing::debug!("gst: appsrc/appsink caps (system memory BGRx → NVMM JPEG)");
        appsrc.set_property("format", gstreamer::Format::Time);
        appsrc.set_property("is-live", true);
        appsrc.set_property("do-timestamp", true);
        appsrc.set_property("max-bytes", 0u64);
        appsrc.set_property("block", false);

        let src_caps = Caps::builder("video/x-raw")
            .field("format", gst_format)
            .field("width", width as i32)
            .field("height", height as i32)
            .field("framerate", Fraction::new(fps as i32, 1))
            .build();
        appsrc.set_caps(Some(&src_caps));

        appsink.set_property("sync", false);
        appsink.set_property("drop", true);
        appsink.set_property("max-buffers", 2u32);
        appsink.set_property("emit-signals", false);
        appsink.set_caps(Some(&Caps::builder("image/jpeg").build()));

        tracing::debug!("gst: adding and linking encode elements");
        pipeline.add_many([
            appsrc.upcast_ref(),
            &nvvidconv,
            &nv12,
            &encoder,
            appsink.upcast_ref(),
        ])?;
        gstreamer::Element::link_many([
            appsrc.upcast_ref(),
            &nvvidconv,
            &nv12,
            &encoder,
            appsink.upcast_ref(),
        ])?;
        tracing::debug!("gst: jpeg encode pipeline linked");

        display.set_codec_config(Some("jpeg".into()), None);

        let frame_tx = display.inner.frame_tx.clone();
        let encoded_once = std::sync::atomic::AtomicBool::new(false);
        appsink.set_callbacks(
            AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| FlowError::Error)?;
                    let buffer = sample.buffer().ok_or(FlowError::Error)?;
                    let map = buffer.map_readable().map_err(|_| FlowError::Error)?;
                    let bytes = map.as_slice().to_vec();
                    if !encoded_once.swap(true, Ordering::Relaxed) {
                        tracing::info!(bytes = bytes.len(), "gst: first jpeg frame");
                    }
                    let _ = frame_tx.send(Arc::new(bytes));
                    Ok(FlowSuccess::Ok)
                })
                .build(),
        );

        Ok(Self {
            pipeline,
            appsrc,
            width,
            height,
            fps,
            drm_fourcc,
        })
    }

    fn set_playing(&self) -> anyhow::Result<()> {
        tracing::debug!("gst: set_state Playing");
        self.pipeline.set_state(State::Playing)?;
        tracing::info!("gst: jpeg encode pipeline Playing");
        Ok(())
    }

    fn set_null(&self) {
        let _ = self.pipeline.set_state(State::Null);
    }

    fn key(&self) -> (u32, u32, u64, u32) {
        (
            self.width,
            self.height,
            self.fps,
            fourcc_u32(self.drm_fourcc),
        )
    }

    fn push_sysmem(&self, pixels: Vec<u8>) -> anyhow::Result<()> {
        let buffer = gstreamer::Buffer::from_mut_slice(pixels);
        self.appsrc
            .push_buffer(buffer)
            .map_err(|e| anyhow::anyhow!("appsrc push failed: {e:?}"))?;
        Ok(())
    }
}

struct OutputInfo {
    output: wl_output::WlOutput,
    name: Option<String>,
}

struct WlState {
    outputs: Vec<OutputInfo>,
    screencopy: Option<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1>,
    shm: Option<wl_shm::WlShm>,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for WlState {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_output::WlOutput, ()> for WlState {
    fn event(
        state: &mut Self,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event {
            if let Some(info) = state.outputs.iter_mut().find(|o| o.output == *output) {
                info.name = Some(name);
            }
        }
    }
}

impl Dispatch<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1, ()> for WlState {
    fn event(
        _: &mut Self,
        _: &zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
        _: zwlr_screencopy_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm::WlShm, ()> for WlState {
    fn event(
        _: &mut Self,
        _: &wl_shm::WlShm,
        _: wl_shm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for WlState {
    fn event(
        _: &mut Self,
        _: &wl_shm_pool::WlShmPool,
        _: wl_shm_pool::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for WlState {
    fn event(
        _: &mut Self,
        _: &wl_buffer::WlBuffer,
        _: wl_buffer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

enum FrameEvent {
    Shm {
        format: u32,
        width: u32,
        height: u32,
        stride: u32,
    },
    BufferDone,
    Ready,
    Failed,
}

impl Dispatch<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1, SyncSender<FrameEvent>> for WlState {
    fn event(
        _: &mut Self,
        proxy: &zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        tx: &SyncSender<FrameEvent>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_screencopy_frame_v1::Event::Buffer {
                format,
                width,
                height,
                stride,
            } => {
                let _ = tx.send(FrameEvent::Shm {
                    format: format.into(),
                    width,
                    height,
                    stride,
                });
            }
            zwlr_screencopy_frame_v1::Event::BufferDone => {
                let _ = tx.send(FrameEvent::BufferDone);
            }
            zwlr_screencopy_frame_v1::Event::Ready { .. } => {
                let _ = tx.send(FrameEvent::Ready);
                proxy.destroy();
            }
            zwlr_screencopy_frame_v1::Event::Failed => {
                let _ = tx.send(FrameEvent::Failed);
                proxy.destroy();
            }
            _ => {}
        }
    }
}

struct CapturedFrame {
    format: DrmFourcc,
    size: (u32, u32),
    pixels: Vec<u8>,
}

struct ShmCapturePool {
    /// Kept alive so the mmap stays valid.
    #[allow(dead_code)]
    file: File,
    map: MappedShm,
    pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
    capacity: usize,
    width: u32,
    height: u32,
    stride: u32,
    wl_fmt: wl_shm::Format,
}

impl ShmCapturePool {
    fn matches(
        &self,
        width: u32,
        height: u32,
        stride: u32,
        wl_fmt: wl_shm::Format,
        size: usize,
    ) -> bool {
        self.width == width
            && self.height == height
            && self.stride == stride
            && self.wl_fmt == wl_fmt
            && self.capacity == size
    }

    fn destroy(self) {
        self.buffer.destroy();
        self.pool.destroy();
        drop(self.map);
    }
}

struct WaylandCapture {
    conn: Connection,
    queue: EventQueue<WlState>,
    state: WlState,
    shm_pool: Option<ShmCapturePool>,
}

impl WaylandCapture {
    fn connect() -> anyhow::Result<Self> {
        apply_wayland_env();
        let conn = Connection::connect_to_env().context("wayland connect")?;
        let (globals, mut queue) = registry_queue_init::<WlState>(&conn)?;
        let qh = queue.handle();

        let mut state = WlState {
            outputs: Vec::new(),
            screencopy: None,
            shm: None,
        };

        let registry = globals.registry().clone();
        for global in globals.contents().clone_list() {
            match global.interface.as_str() {
                "wl_output" => {
                    let version = global.version.clamp(1, 4);
                    let output =
                        registry.bind::<wl_output::WlOutput, _, _>(global.name, version, &qh, ());
                    state.outputs.push(OutputInfo { output, name: None });
                }
                "zwlr_screencopy_manager_v1" => {
                    // wayland-protocols-wlr 0.3 XML max is v3; wlroots advertises v4.
                    let mgr = globals
                        .bind::<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1, _, _>(
                            &qh,
                            1..=3,
                            (),
                        )?;
                    state.screencopy = Some(mgr);
                }
                "wl_shm" => {
                    let shm = registry.bind::<wl_shm::WlShm, _, _>(global.name, 1, &qh, ());
                    state.shm = Some(shm);
                }
                _ => {}
            }
        }

        queue.roundtrip(&mut state)?;
        queue.roundtrip(&mut state)?;

        if state.screencopy.is_none() {
            anyhow::bail!("zwlr_screencopy_manager_v1 missing");
        }
        if state.shm.is_none() {
            anyhow::bail!("wl_shm missing");
        }

        Ok(Self {
            conn,
            queue,
            state,
            shm_pool: None,
        })
    }

    fn output_by_name(&self, name: &str) -> Option<&wl_output::WlOutput> {
        self.state
            .outputs
            .iter()
            .find(|o| o.name.as_deref() == Some(name))
            .map(|o| &o.output)
    }

    fn capture_shm(&mut self, output_name: &str) -> anyhow::Result<CapturedFrame> {
        let output = self
            .output_by_name(output_name)
            .ok_or_else(|| anyhow::anyhow!("wayland output {output_name} not found"))?
            .clone();
        let mgr = self
            .state
            .screencopy
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("screencopy manager missing"))?
            .clone();
        let shm = self
            .state
            .shm
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("wl_shm missing"))?
            .clone();

        let (tx, rx) = mpsc::sync_channel::<FrameEvent>(8);
        let frame = mgr.capture_output(
            0, /* overlay_cursor: compositor pointer stays out of the JPEG */
            &output,
            &self.queue.handle(),
            tx,
        );
        self.flush()?;

        let mut shm_params = None;
        loop {
            self.dispatch()?;
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    FrameEvent::Shm {
                        format,
                        width,
                        height,
                        stride,
                    } => shm_params = Some((format, width, height, stride)),
                    FrameEvent::BufferDone => {
                        let (format, width, height, stride) = shm_params.ok_or_else(|| {
                            anyhow::anyhow!("screencopy frame missing wl_shm buffer params")
                        })?;
                        return self.copy_shm_frame(
                            shm,
                            &frame,
                            &rx,
                            format,
                            (width, height),
                            stride,
                        );
                    }
                    FrameEvent::Failed => anyhow::bail!("screencopy frame failed"),
                    FrameEvent::Ready => anyhow::bail!("screencopy ready before copy"),
                }
            }
        }
    }

    fn ensure_shm_pool(
        &mut self,
        shm: &wl_shm::WlShm,
        width: u32,
        height: u32,
        stride: u32,
        wl_fmt: wl_shm::Format,
        size: usize,
    ) -> anyhow::Result<()> {
        if self
            .shm_pool
            .as_ref()
            .is_some_and(|p| p.matches(width, height, stride, wl_fmt, size))
        {
            return Ok(());
        }
        if let Some(old) = self.shm_pool.take() {
            old.destroy();
        }
        let xdg = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
        let path = PathBuf::from(&xdg).join(format!("how-scrcpy-{}", std::process::id()));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .with_context(|| format!("shm file {}", path.display()))?;
        let _ = std::fs::remove_file(&path);
        file.set_len(size as u64)?;
        let map = MappedShm::map(&file, size)?;
        let pool = shm.create_pool(file.as_fd(), size as i32, &self.queue.handle(), ());
        let buffer = pool.create_buffer(
            0,
            width as i32,
            height as i32,
            stride as i32,
            wl_fmt,
            &self.queue.handle(),
            (),
        );
        self.shm_pool = Some(ShmCapturePool {
            file,
            map,
            pool,
            buffer,
            capacity: size,
            width,
            height,
            stride,
            wl_fmt,
        });
        Ok(())
    }

    fn copy_shm_frame(
        &mut self,
        shm: wl_shm::WlShm,
        frame: &zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
        rx: &Receiver<FrameEvent>,
        shm_format: u32,
        (width, height): (u32, u32),
        stride: u32,
    ) -> anyhow::Result<CapturedFrame> {
        let drm = drm_fourcc_for_wl_shm(shm_format)
            .ok_or_else(|| anyhow::anyhow!("unsupported wl_shm format {shm_format:#x}"))?;
        let wl_fmt = wl_shm_format_for_drm(drm)
            .ok_or_else(|| anyhow::anyhow!("unsupported drm format {drm:?}"))?;
        let src_bpp = match drm {
            DrmFourcc::Bgr888 | DrmFourcc::Rgb888 => 3,
            _ => 4,
        };
        let size = (stride as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| anyhow::anyhow!("shm size overflow"))?;
        if size > i32::MAX as usize {
            anyhow::bail!("shm buffer too large ({size})");
        }
        self.ensure_shm_pool(&shm, width, height, stride, wl_fmt, size)?;
        let wl_buffer = self
            .shm_pool
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("screencopy shm pool missing"))?
            .buffer
            .clone();
        frame.copy(&wl_buffer);
        self.flush()?;

        loop {
            self.dispatch()?;
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    FrameEvent::Ready => {
                        let pool = self
                            .shm_pool
                            .as_ref()
                            .ok_or_else(|| anyhow::anyhow!("screencopy shm pool missing"))?;
                        let mapped = pool.map.bytes(size)?;
                        let swap_rb = matches!(
                            drm,
                            DrmFourcc::Bgr888 | DrmFourcc::Xbgr8888 | DrmFourcc::Abgr8888
                        );
                        let row = (width as usize).saturating_mul(src_bpp);
                        let pixels = if src_bpp == 4 && !swap_rb && stride as usize == row {
                            let total = row.saturating_mul(height as usize);
                            mapped[..total].to_vec()
                        } else {
                            pack_to_bgrx(mapped, stride, width, height, src_bpp, swap_rb)?
                        };
                        return Ok(CapturedFrame {
                            format: DrmFourcc::Xrgb8888,
                            size: (width, height),
                            pixels,
                        });
                    }
                    FrameEvent::Failed => anyhow::bail!("screencopy copy failed"),
                    _ => {}
                }
            }
        }
    }

    fn dispatch(&mut self) -> anyhow::Result<()> {
        self.queue.blocking_dispatch(&mut self.state)?;
        Ok(())
    }

    fn flush(&mut self) -> anyhow::Result<()> {
        self.conn.flush()?;
        Ok(())
    }
}

fn run_capture_loop(display: DisplayState) -> anyhow::Result<()> {
    gstreamer::init().context("gstreamer init")?;

    let gate = display.inner.capture_gate.subscribe();
    let mut last_err_log = Instant::now() - Duration::from_secs(60);
    let mut next_frame = Instant::now();
    let mut logged_ready = false;
    let mut wl: Option<WaylandCapture> = None;
    let mut pipeline: Option<EncodePipeline> = None;
    let mut last_size = (0u32, 0u32);
    let mut last_format = DRM_FORMAT_XRGB8888;
    let mut retry_after = Instant::now();
    let mut last_gate_active = *gate.borrow();
    let mut last_stream_target_write = Instant::now() - Duration::from_secs(60);
    let stream_target_min_interval = Duration::from_millis(500);

    let mut write_stream_target_throttled = |force: bool| {
        let now = Instant::now();
        if force || now.duration_since(last_stream_target_write) >= stream_target_min_interval {
            let _ = write_stream_target(&display);
            last_stream_target_write = now;
        }
    };

    loop {
        let gate_active = *gate.borrow();
        if gate_active != last_gate_active {
            last_gate_active = gate_active;
            write_stream_target_throttled(true);
        }
        if !gate_active {
            logged_ready = false;
            if let Some(p) = pipeline.take() {
                p.set_null();
            }
            wl = None;
            thread::sleep(Duration::from_millis(200));
            continue;
        }

        if Instant::now() < retry_after {
            thread::sleep(Duration::from_millis(200));
            continue;
        }

        let preferred = display.inner.viz_output.lock().clone();
        let Some(output_name) = preferred else {
            thread::sleep(Duration::from_millis(500));
            continue;
        };

        let (src_w, src_h) = *display.inner.source_size.lock();
        let (dw, dh) = crate::display::native_stream_size(src_w, src_h);
        *display.inner.scaled_size.lock() = (dw, dh);
        write_stream_target_throttled(false);
        let fps = display.max_framerate().clamp(1, 20);

        if wl.is_none() {
            match WaylandCapture::connect() {
                Ok(w) => {
                    wl = Some(w);
                    if !logged_ready {
                        tracing::info!("screencopy using zwlr_screencopy wl_shm + nvjpegenc");
                        logged_ready = true;
                    }
                }
                Err(e) => {
                    if last_err_log.elapsed() >= Duration::from_secs(10) {
                        tracing::warn!(?e, "screencopy wayland setup failed");
                        last_err_log = Instant::now();
                    }
                    display.set_error(Some(e.to_string()));
                    retry_after = Instant::now() + Duration::from_secs(5);
                    continue;
                }
            }
        }

        let drm_fourcc = DrmFourcc::try_from(last_format).unwrap_or(DrmFourcc::Xrgb8888);
        let need_pipeline = pipeline.as_ref().map(|p| p.key()) != Some((dw, dh, fps, last_format));

        if need_pipeline {
            if let Some(p) = pipeline.take() {
                p.set_null();
            }
            display.set_codec_config(None, None);
            match EncodePipeline::new(&display, dw, dh, fps, drm_fourcc) {
                Ok(p) => {
                    if let Err(e) = p.set_playing() {
                        if last_err_log.elapsed() >= Duration::from_secs(10) {
                            tracing::warn!(?e, "screencopy pipeline play failed");
                            last_err_log = Instant::now();
                        }
                        display.set_error(Some(format!(
                            "JPEG pipeline failed (need host L4T GStreamer plugins): {e}"
                        )));
                        retry_after = Instant::now() + Duration::from_secs(5);
                        continue;
                    }
                    display.set_error(None);
                    pipeline = Some(p);
                    last_size = (dw, dh);
                }
                Err(e) => {
                    if last_err_log.elapsed() >= Duration::from_secs(10) {
                        tracing::warn!(?e, "screencopy pipeline build failed");
                        last_err_log = Instant::now();
                    }
                    display.set_error(Some(e.to_string()));
                    retry_after = Instant::now() + Duration::from_secs(5);
                    continue;
                }
            }
        }

        let now = Instant::now();
        if now < next_frame {
            thread::sleep(next_frame - now);
            continue;
        }
        next_frame = Instant::now() + Duration::from_millis(1000 / fps);

        let started = Instant::now();
        let mut reset_wl = false;
        let capture_result = wl.as_mut().unwrap().capture_shm(&output_name);
        match capture_result {
            Ok(captured) => {
                last_format = fourcc_u32(captured.format);
                // Output moves can race the cached size; never encode with stale caps.
                if captured.size != (dw, dh) {
                    *display.inner.source_size.lock() = captured.size;
                    write_stream_target_throttled(true);
                    continue;
                }
                if let Some(p) = pipeline.as_ref() {
                    if let Err(e) = p.push_sysmem(captured.pixels) {
                        if last_err_log.elapsed() >= Duration::from_secs(10) {
                            tracing::warn!(?e, "screencopy appsrc push failed");
                            last_err_log = Instant::now();
                        }
                        display.set_error(Some(e.to_string()));
                    } else {
                        display
                            .inner
                            .last_capture_ms
                            .store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
                        display.set_error(None);
                        write_stream_target_throttled(true);
                    }
                }
            }
            Err(e) => {
                if last_err_log.elapsed() >= Duration::from_secs(10) {
                    tracing::warn!(?e, "screencopy capture failed");
                    last_err_log = Instant::now();
                }
                display.set_error(Some(e.to_string()));
                if last_size != (dw, dh) {
                    reset_wl = true;
                }
            }
        }
        if reset_wl {
            wl = None;
        }
    }
}
