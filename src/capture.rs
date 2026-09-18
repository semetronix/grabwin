use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use windows::core::{IInspectable, HSTRING};
use windows::Foundation::Metadata::ApiInformation;
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Graphics::Direct3D11::{ID3D11Texture2D, D3D11_BOX};
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

use crate::d3d::{self, D3D};
use crate::error::{Error, Result};
use crate::frame::Frame;
use crate::window::{self, Crop};

const PIXEL_FORMAT: DirectXPixelFormat = DirectXPixelFormat::B8G8R8A8UIntNormalized;
const POOL_FRAMES: i32 = 2;

/// WGC delivers exactly one frame when a static window is resized, and none after
/// `Direct3D11CaptureFramePool::Recreate` (verified empirically). That frame's surface has the
/// *pool's* size, so it is only fully usable if the pool is already at least as large as the new
/// content. The pool is therefore sized to the window's monitor (or the content, if larger), which
/// makes every resize within the monitor usable immediately; only growth beyond the pool needs a
/// recreate, after which the next grab may time out until the window repaints.
fn pool_size_for(hwnd: isize, content: SizeInt32) -> SizeInt32 {
    let (mon_w, mon_h) = match window::monitor_of(hwnd) {
        Ok((_, (l, t, r, b))) => (r - l, b - t),
        Err(_) => (0, 0),
    };
    SizeInt32 {
        Width: content.Width.max(mon_w),
        Height: content.Height.max(mon_h),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    OnDemand,
    Live,
}

/// What the WGC item was created for. `Monitor` is the fallback for exclusive-fullscreen windows,
/// which WGC cannot capture as windows; frames are then cropped to the window's client rect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Window,
    Monitor,
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub mode: Mode,
    pub timeout_ms: u32,
    pub cursor: bool,
    pub border: bool,
}

/// The crop-sized GPU copy of the last frame and its CPU-readable staging twin. Always created and
/// dropped together: a half-initialised pair would leave the next frame with nothing to read back.
struct Textures {
    latest: ID3D11Texture2D,
    staging: ID3D11Texture2D,
}

impl Textures {
    fn create(d3d: &D3D, width: u32, height: u32) -> Result<Self> {
        let latest = d3d::create_texture(&d3d.device, width, height, false)?;
        let staging = d3d::create_texture(&d3d.device, width, height, true)?;
        Ok(Self { latest, staging })
    }
}

struct State {
    /// Surface size of the frame pool; frames are only usable while content fits in it.
    pool: (i32, i32),
    /// Last seen `ContentSize`; `crop` is valid for this size.
    content: (i32, i32),
    crop: Crop,
    textures: Option<Textures>,
    latest_cpu: Option<Vec<u8>>,
}

impl State {
    fn has_frame(&self, mode: Mode) -> bool {
        match mode {
            Mode::OnDemand => self.textures.is_some(),
            Mode::Live => self.latest_cpu.is_some(),
        }
    }
}

/// Locks `m`, ignoring poisoning. A panic while a guard was held (caught by pyo3 on a Python
/// thread, or by the frame callback's `catch_unwind`) must not turn every later lock into another
/// panic - inside the WGC callback that would abort the process.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Shared {
    hwnd: isize,
    d3d: D3D,
    mode: Mode,
    target: Target,
    /// Top-left of the captured monitor in screen coordinates; `(0, 0)` for `Target::Window`.
    monitor_origin: (i32, i32),
    state: Mutex<State>,
    cv: Condvar,
    closed: AtomicBool,
    /// Last failure of the frame callback, surfaced by the next `grab_bgra`. The callback must never
    /// log (pyo3-log takes the GIL), so this is its only way to report.
    last_error: Mutex<Option<Error>>,
}

// SAFETY: `windows` 0.62 does not mark COM/WinRT interfaces Send/Sync. Everything held here is
// created on a free-threaded apartment (D3D11 device/context with ID3D11Multithread protection
// enabled, a CreateFreeThreaded frame pool, agile WinRT capture objects) and is documented by
// Microsoft as safe to call from any thread; mutable state lives behind `Mutex`/atomics.
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

pub struct Capture {
    shared: Arc<Shared>,
    item: GraphicsCaptureItem,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    frame_token: i64,
    closed_token: i64,
    timeout_ms: u32,
    stopped: bool,
}

// SAFETY: see `Shared`; the item/pool/session are free-threaded WinRT objects.
unsafe impl Send for Capture {}

fn property_present(name: &str) -> bool {
    ApiInformation::IsPropertyPresent(
        &HSTRING::from("Windows.Graphics.Capture.GraphicsCaptureSession"),
        &HSTRING::from(name),
    )
    .unwrap_or(false)
}

/// True when the window (by either rect) exactly covers its monitor - the shape of an
/// exclusive-fullscreen game, and also of a borderless window sized to the screen.
fn is_fullscreen(hwnd: isize) -> Result<bool> {
    let geo = window::geometry(hwnd)?;
    let (_, mon) = window::monitor_of(hwnd)?;
    Ok(geo.window == mon || geo.extended == mon)
}

/// Client-rect crop of a `w`x`h` frame for the given target. Runs on the WGC callback thread
/// (see `on_frame`), so it must never log.
fn compute_crop(
    hwnd: isize,
    target: Target,
    monitor_origin: (i32, i32),
    w: i32,
    h: i32,
) -> Result<Crop> {
    match target {
        Target::Window => window::pick_crop(hwnd, w, h),
        Target::Monitor => {
            // Client rect in screen coords relative to the monitor origin, clamped to the monitor.
            let geo = window::geometry(hwnd)?;
            Ok(window::clamp_client_crop(&geo, monitor_origin, w, h))
        }
    }
}

/// Runs on the WGC worker thread. MUST NOT log or otherwise touch Python: a Python thread may be
/// blocked on `state` while holding the GIL, and pyo3-log acquires the GIL to emit a record.
fn on_frame(shared: &Shared, pool: &Direct3D11CaptureFramePool) -> Result<()> {
    // A null frame (pool already drained) or a closed pool are not worth reporting.
    let Ok(frame) = pool.TryGetNextFrame() else {
        return Ok(());
    };
    let size = frame.ContentSize()?;
    if size.Width <= 0 || size.Height <= 0 {
        return Ok(());
    }
    // A minimized window shrinks to a stub rect at (-32000, -32000); any frame WGC sends in that
    // state must not trigger a pool recreate (which would drop the buffered last frame).
    if window::is_minimized(shared.hwnd) {
        return Ok(());
    }
    let mut st = lock(&shared.state);

    if size.Width > st.pool.0 || size.Height > st.pool.1 {
        // Content outgrew the pool: this frame's surface holds only part of it. Recreate larger and
        // drop the frame (see `pool_size_for` for why this is a last resort). Compute the new crop
        // before touching anything so a failure leaves the state consistent.
        drop(frame);
        let crop = compute_crop(
            shared.hwnd,
            shared.target,
            shared.monitor_origin,
            size.Width,
            size.Height,
        )?;
        let pool_size = pool_size_for(shared.hwnd, size);
        pool.Recreate(&shared.d3d.winrt, PIXEL_FORMAT, POOL_FRAMES, pool_size)?;
        st.pool = (pool_size.Width, pool_size.Height);
        st.content = (size.Width, size.Height);
        st.crop = crop;
        st.textures = None;
        st.latest_cpu = None;
        return Ok(());
    }

    if (size.Width, size.Height) != st.content {
        // Resized within the pool: the content sits at the surface's top-left, so this very frame
        // is usable with a fresh crop; the cached textures have the old crop size.
        let crop = compute_crop(
            shared.hwnd,
            shared.target,
            shared.monitor_origin,
            size.Width,
            size.Height,
        )?;
        st.content = (size.Width, size.Height);
        st.crop = crop;
        st.textures = None;
        st.latest_cpu = None;
    }

    let src = d3d::texture_from_surface(&frame.Surface()?)?;
    let crop = st.crop;
    if st.textures.is_none() {
        // Built into a local first: on failure the state stays "no textures" rather than half set.
        st.textures = Some(Textures::create(&shared.d3d, crop.width, crop.height)?);
    }
    let region = D3D11_BOX {
        left: crop.x,
        top: crop.y,
        front: 0,
        right: crop.x + crop.width,
        bottom: crop.y + crop.height,
        back: 1,
    };
    let Some(tex) = st.textures.as_ref() else {
        return Err(Error::Internal(
            "frame textures missing after creation".into(),
        ));
    };
    unsafe {
        shared
            .d3d
            .context
            .CopySubresourceRegion(&tex.latest, 0, 0, 0, 0, &src, 0, Some(&region));
    }
    if shared.mode == Mode::Live {
        let cpu = d3d::readback(
            &shared.d3d.context,
            &tex.staging,
            &tex.latest,
            crop.width,
            crop.height,
        )?;
        st.latest_cpu = Some(cpu);
    }
    drop(st);
    shared.cv.notify_all();
    Ok(())
}

/// Best-effort text of a panic payload (`panic!("..")` gives `&str` or `String`).
fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s
    } else {
        "<non-string payload>"
    }
}

impl Capture {
    /// Window capture first; if the window is monitor-sized and yields no frame within the timeout,
    /// switch to capturing the monitor it sits on (exclusive-fullscreen games). Must be called
    /// without the GIL: it logs, and for a monitor-sized window it may block for up to
    /// `opts.timeout_ms` (the probe grab) plus the time to start the second, monitor session.
    pub fn start_with_fallback(hwnd: isize, opts: Options) -> Result<Self> {
        let mut cap = Self::start_inner(hwnd, opts, Target::Window)?;
        if !is_fullscreen(hwnd)? {
            return Ok(cap);
        }
        match cap.grab_bgra() {
            Ok(_) => Ok(cap),
            Err(Error::Timeout(_)) => {
                log::debug!("fullscreen window gave no frame; falling back to monitor capture");
                cap.close();
                Self::start_inner(hwnd, opts, Target::Monitor)
            }
            Err(e) => Err(e),
        }
    }

    /// Must be called without the GIL: it logs, and pyo3-log acquires the GIL to do so.
    fn start_inner(hwnd: isize, opts: Options, target: Target) -> Result<Self> {
        if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
            return Err(Error::Unsupported(
                "Windows Graphics Capture is not available (need Windows 10 1903+)".into(),
            ));
        }
        let d3d = d3d::create()?;
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let (item, monitor_origin): (GraphicsCaptureItem, (i32, i32)) = match target {
            Target::Window => (
                unsafe { interop.CreateForWindow(window::hwnd(hwnd))? },
                (0, 0),
            ),
            Target::Monitor => {
                let (hmon, rect) = window::monitor_of(hwnd)?;
                let item =
                    unsafe { interop.CreateForMonitor(HMONITOR(hmon as *mut std::ffi::c_void))? };
                (item, (rect.0, rect.1))
            }
        };
        let size: SizeInt32 = item.Size()?;
        let crop = compute_crop(hwnd, target, monitor_origin, size.Width, size.Height)?;
        let pool_size = pool_size_for(hwnd, size);
        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "crop for hwnd {hwnd} ({target:?}): {crop:?} (content {}x{}, pool {}x{}, geo {:?})",
                size.Width,
                size.Height,
                pool_size.Width,
                pool_size.Height,
                window::geometry(hwnd)
            );
        }

        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &d3d.winrt,
            PIXEL_FORMAT,
            POOL_FRAMES,
            pool_size,
        )?;
        let session = pool.CreateCaptureSession(&item)?;

        let shared = Arc::new(Shared {
            hwnd,
            d3d,
            mode: opts.mode,
            target,
            monitor_origin,
            state: Mutex::new(State {
                pool: (pool_size.Width, pool_size.Height),
                content: (size.Width, size.Height),
                crop,
                textures: None,
                latest_cpu: None,
            }),
            cv: Condvar::new(),
            closed: AtomicBool::new(false),
            last_error: Mutex::new(None),
        });

        // Neither handler may log or touch Python (see `on_frame`). `Invoke` is `extern "system"`,
        // so a panic escaping this closure would abort the whole (Python) process: catch it and
        // report it like any other callback failure.
        let frame_token = pool.FrameArrived(&TypedEventHandler::<
            Direct3D11CaptureFramePool,
            IInspectable,
        >::new({
            let shared = shared.clone();
            move |pool, _| {
                let Some(pool) = pool.as_ref() else {
                    return Ok(());
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    on_frame(&shared, pool)
                }));
                let err = match result {
                    Ok(Ok(())) => return Ok(()),
                    Ok(Err(e)) => e,
                    Err(payload) => Error::Internal(format!(
                        "frame handler panicked: {}",
                        panic_message(payload.as_ref())
                    )),
                };
                *lock(&shared.last_error) = Some(err);
                shared.cv.notify_all();
                Ok(())
            }
        }))?;

        let closed_token = item.Closed(
            &TypedEventHandler::<GraphicsCaptureItem, IInspectable>::new({
                let shared = shared.clone();
                move |_, _| {
                    shared.closed.store(true, Ordering::SeqCst);
                    shared.cv.notify_all();
                    Ok(())
                }
            }),
        )?;

        if property_present("IsCursorCaptureEnabled") {
            if let Err(e) = session.SetIsCursorCaptureEnabled(opts.cursor) {
                log::warn!("cannot set cursor capture: {e}");
            }
        }
        if !opts.border && property_present("IsBorderRequired") {
            if let Err(e) = session.SetIsBorderRequired(false) {
                log::warn!("cannot disable capture border: {e}");
            }
        }

        session.StartCapture()?;
        Ok(Self {
            shared,
            item,
            pool,
            session,
            frame_token,
            closed_token,
            timeout_ms: opts.timeout_ms,
            stopped: false,
        })
    }

    pub fn is_closed(&self) -> bool {
        self.stopped || self.shared.closed.load(Ordering::SeqCst)
    }

    pub fn target(&self) -> Target {
        self.shared.target
    }

    pub fn size(&self) -> (u32, u32) {
        let st = lock(&self.shared.state);
        (st.crop.width, st.crop.height)
    }

    /// Blocks (without the GIL - caller's responsibility) until a frame is available, then returns it.
    pub fn grab_bgra(&self) -> Result<Frame> {
        let sh = &self.shared;
        // Check liveness up front: the WGC Closed event may lag behind the window's destruction,
        // and a buffered frame must never be returned for a window that no longer exists.
        if self.is_closed() || !window::is_alive(sh.hwnd) {
            return Err(Error::Closed);
        }
        if let Some(e) = lock(&sh.last_error).take() {
            return Err(e);
        }
        let deadline = Instant::now() + Duration::from_millis(self.timeout_ms as u64);
        let mut st = lock(&sh.state);
        while !st.has_frame(sh.mode) {
            if sh.closed.load(Ordering::SeqCst) {
                return Err(Error::Closed);
            }
            if let Some(e) = lock(&sh.last_error).take() {
                return Err(e);
            }
            if window::is_minimized(sh.hwnd) {
                return Err(Error::Minimized);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::Timeout(self.timeout_ms));
            }
            let (guard, _) = sh
                .cv
                .wait_timeout(st, deadline - now)
                .unwrap_or_else(PoisonError::into_inner);
            st = guard;
        }
        let crop = st.crop;
        // `has_frame` guarantees the source below is present; fail softly rather than unwrap.
        let missing = || Error::Internal("frame vanished while the state was locked".into());
        let bgra = match sh.mode {
            Mode::Live => st.latest_cpu.clone().ok_or_else(missing)?,
            Mode::OnDemand => {
                let tex = st.textures.as_ref().ok_or_else(missing)?;
                d3d::readback(
                    &sh.d3d.context,
                    &tex.staging,
                    &tex.latest,
                    crop.width,
                    crop.height,
                )?
            }
        };
        Ok(Frame {
            bgra,
            width: crop.width,
            height: crop.height,
        })
    }

    /// Safe to call with the GIL held (Python dealloc path): it never logs or blocks on Python.
    pub fn close(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        let _ = self.pool.RemoveFrameArrived(self.frame_token);
        let _ = self.item.RemoveClosed(self.closed_token);
        let _ = self.session.Close();
        let _ = self.pool.Close();
        self.shared.cv.notify_all();
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_survives_poisoning() {
        let m = Arc::new(Mutex::new(5));
        let poisoner = m.clone();
        let _ = std::thread::spawn(move || {
            let _g = poisoner.lock().unwrap();
            panic!("poison");
        })
        .join();
        assert!(m.is_poisoned());
        assert_eq!(*lock(&m), 5);
        *lock(&m) = 6;
        assert_eq!(*lock(&m), 6);
    }

    #[test]
    fn panic_payload_is_reported() {
        let r = std::panic::catch_unwind(|| panic!("static str"));
        assert_eq!(panic_message(r.unwrap_err().as_ref()), "static str");
        let r = std::panic::catch_unwind(|| panic!("{}", String::from("formatted")));
        assert_eq!(panic_message(r.unwrap_err().as_ref()), "formatted");
        let r = std::panic::catch_unwind(|| std::panic::panic_any(7u8));
        assert_eq!(
            panic_message(r.unwrap_err().as_ref()),
            "<non-string payload>"
        );
    }
}
