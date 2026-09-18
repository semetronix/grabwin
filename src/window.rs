use std::ffi::c_void;

use pyo3::prelude::*;
use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::HiDpi::{
    SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClientRect, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, GWL_EXSTYLE,
    WS_EX_TOOLWINDOW,
};

use crate::error::{Error, Result};

pub fn hwnd(h: isize) -> HWND {
    HWND(h as *mut c_void)
}

/// Switches the *current thread* to per-monitor-v2 DPI awareness for the guard's lifetime,
/// so GetWindowRect/GetClientRect return physical pixels. Process-wide awareness is untouched.
struct DpiGuard(DPI_AWARENESS_CONTEXT);

impl DpiGuard {
    fn new() -> Self {
        Self(unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) })
    }
}

impl Drop for DpiGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                SetThreadDpiAwarenessContext(self.0);
            }
        }
    }
}

#[pyclass(module = "screenshot_helper", get_all, frozen)]
#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub hwnd: isize,
    pub title: String,
    pub process: String,
    pub pid: u32,
    pub width: i32,
    pub height: i32,
    pub is_minimized: bool,
}

#[pymethods]
impl WindowInfo {
    fn __repr__(&self) -> String {
        format!(
            "WindowInfo(hwnd={}, title={:?}, process={:?}, pid={}, width={}, height={}, is_minimized={})",
            self.hwnd, self.title, self.process, self.pid, self.width, self.height, self.is_minimized
        )
    }
}

#[derive(Debug, Clone)]
pub enum Selector {
    Hwnd(isize),
    Title(String),
    Process(String),
}

/// All rects are (left, top, right, bottom) in physical screen pixels.
#[derive(Debug, Clone, Copy)]
pub struct Geometry {
    pub window: (i32, i32, i32, i32),
    pub extended: (i32, i32, i32, i32),
    pub client_origin: (i32, i32),
    pub client_size: (i32, i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

unsafe extern "system" fn enum_cb(h: HWND, lparam: LPARAM) -> BOOL {
    let out = &mut *(lparam.0 as *mut Vec<isize>);
    out.push(h.0 as isize);
    BOOL(1)
}

fn is_capturable(h: isize) -> bool {
    let w = hwnd(h);
    unsafe {
        if !IsWindowVisible(w).as_bool() || GetWindowTextLengthW(w) == 0 {
            return false;
        }
        if (GetWindowLongPtrW(w, GWL_EXSTYLE) as u32) & WS_EX_TOOLWINDOW.0 != 0 {
            return false;
        }
        let mut cloaked: u32 = 0;
        let ok = DwmGetWindowAttribute(
            w,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut c_void,
            std::mem::size_of::<u32>() as u32,
        )
        .is_ok();
        if ok && cloaked != 0 {
            return false;
        }
    }
    true
}

fn window_title(h: isize) -> String {
    let w = hwnd(h);
    let len = unsafe { GetWindowTextLengthW(w) };
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = unsafe { GetWindowTextW(w, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn process_name(pid: u32) -> String {
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new(); // elevated / protected process: no name, not an error
        };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        if !ok {
            return String::new();
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        full.rsplit(['\\', '/']).next().unwrap_or("").to_string()
    }
}

/// Requires the caller to hold a DpiGuard.
fn info_unguarded(h: isize) -> Option<WindowInfo> {
    let w = hwnd(h);
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(w, Some(&mut pid)) };
    let mut client = RECT::default();
    unsafe { GetClientRect(w, &mut client).ok()? };
    Some(WindowInfo {
        hwnd: h,
        title: window_title(h),
        process: process_name(pid),
        pid,
        width: client.right - client.left,
        height: client.bottom - client.top,
        is_minimized: unsafe { IsIconic(w).as_bool() },
    })
}

/// Visible, titled, non-tool, non-cloaked top-level windows in Z-order (topmost first).
pub fn list_windows() -> Vec<WindowInfo> {
    let mut handles: Vec<isize> = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(enum_cb),
            LPARAM(&mut handles as *mut Vec<isize> as isize),
        );
    }
    let _dpi = DpiGuard::new();
    handles
        .into_iter()
        .filter(|&h| is_capturable(h))
        .filter_map(info_unguarded)
        .collect()
}

pub fn find_window(sel: &Selector) -> Result<WindowInfo> {
    match sel {
        Selector::Hwnd(h) => {
            if !is_alive(*h) {
                return Err(Error::NotFound(format!("hwnd {h} is not a window")));
            }
            let _dpi = DpiGuard::new();
            info_unguarded(*h).ok_or_else(|| Error::NotFound(format!("hwnd {h}")))
        }
        Selector::Title(t) => {
            let needle = t.to_lowercase();
            list_windows()
                .into_iter()
                .find(|w| w.title.to_lowercase().contains(&needle))
                .ok_or_else(|| Error::NotFound(format!("no window with title containing {t:?}")))
        }
        Selector::Process(p) => {
            let needle = p.to_lowercase();
            list_windows()
                .into_iter()
                .filter(|w| w.process.to_lowercase() == needle)
                .max_by_key(|w| w.width as i64 * w.height as i64)
                .ok_or_else(|| Error::NotFound(format!("no window owned by process {p:?}")))
        }
    }
}

pub fn is_minimized(h: isize) -> bool {
    unsafe { IsIconic(hwnd(h)).as_bool() }
}

pub fn is_alive(h: isize) -> bool {
    unsafe { IsWindow(Some(hwnd(h))).as_bool() }
}

fn rect_tuple(r: RECT) -> (i32, i32, i32, i32) {
    (r.left, r.top, r.right, r.bottom)
}

pub fn geometry(h: isize) -> Result<Geometry> {
    let _dpi = DpiGuard::new();
    let w = hwnd(h);
    let mut window = RECT::default();
    let mut client = RECT::default();
    let mut extended = RECT::default();
    let mut origin = POINT::default();
    unsafe {
        GetWindowRect(w, &mut window)?;
        GetClientRect(w, &mut client)?;
        if DwmGetWindowAttribute(
            w,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut extended as *mut RECT as *mut c_void,
            std::mem::size_of::<RECT>() as u32,
        )
        .is_err()
        {
            extended = window;
        }
        if !ClientToScreen(w, &mut origin).as_bool() {
            return Err(Error::Windows(windows::core::Error::from_thread()));
        }
    }
    Ok(Geometry {
        window: rect_tuple(window),
        extended: rect_tuple(extended),
        client_origin: (origin.x, origin.y),
        client_size: (client.right - client.left, client.bottom - client.top),
    })
}

/// Chooses the frame origin (GetWindowRect vs DWM extended bounds) by matching the WGC content
/// size, then returns the client-area crop clamped to the content. Pure; unit-tested. Never logs.
pub fn crop_for(geo: &Geometry, content_w: i32, content_h: i32) -> Crop {
    let win_size = (geo.window.2 - geo.window.0, geo.window.3 - geo.window.1);
    let ext_size = (
        geo.extended.2 - geo.extended.0,
        geo.extended.3 - geo.extended.1,
    );
    let origin = if (content_w, content_h) == win_size {
        (geo.window.0, geo.window.1)
    } else if (content_w, content_h) == ext_size {
        (geo.extended.0, geo.extended.1)
    } else {
        // Matches neither; fall back to the GetWindowRect origin. (No logging here: this runs on
        // the WGC callback thread, and pyo3-log would take the GIL.)
        (geo.window.0, geo.window.1)
    };
    clamp_client_crop(geo, origin, content_w, content_h)
}

/// Client-area crop of a `content_w`x`content_h` frame whose top-left sits at `origin` in screen
/// coordinates (a window frame origin or a monitor origin), clamped to the frame and never empty.
/// Pure; never logs.
pub fn clamp_client_crop(
    geo: &Geometry,
    origin: (i32, i32),
    content_w: i32,
    content_h: i32,
) -> Crop {
    let x = (geo.client_origin.0 - origin.0).clamp(0, (content_w - 1).max(0));
    let y = (geo.client_origin.1 - origin.1).clamp(0, (content_h - 1).max(0));
    let width = geo.client_size.0.clamp(1, (content_w - x).max(1));
    let height = geo.client_size.1.clamp(1, (content_h - y).max(1));
    Crop {
        x: x as u32,
        y: y as u32,
        width: width as u32,
        height: height as u32,
    }
}

/// Called from the WGC frame callback (with the capture state locked), so it must never log:
/// pyo3-log acquires the GIL, which a Python thread waiting on that lock may be holding.
pub fn pick_crop(h: isize, content_w: i32, content_h: i32) -> Result<Crop> {
    Ok(crop_for(&geometry(h)?, content_w, content_h))
}

/// Returns the HMONITOR (as isize) and its rect for the monitor nearest to the window.
pub fn monitor_of(h: isize) -> Result<(isize, (i32, i32, i32, i32))> {
    let _dpi = DpiGuard::new();
    let mon = unsafe { MonitorFromWindow(hwnd(h), MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(mon, &mut info) }.as_bool() {
        return Err(Error::Windows(windows::core::Error::from_thread()));
    }
    Ok((mon.0 as isize, rect_tuple(info.rcMonitor)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geo() -> Geometry {
        Geometry {
            // GetWindowRect: includes 7px invisible borders on left/right/bottom
            window: (93, 100, 907, 707),
            // DWM extended frame bounds: visible frame only
            extended: (100, 100, 900, 700),
            // client area starts below a 30px title bar
            client_origin: (100, 130),
            client_size: (800, 570),
        }
    }

    #[test]
    fn crop_when_content_matches_window_rect() {
        // content = GetWindowRect size (814x607) -> origin is window.left/top
        let c = crop_for(&geo(), 814, 607);
        assert_eq!(
            c,
            Crop {
                x: 7,
                y: 30,
                width: 800,
                height: 570
            }
        );
    }

    #[test]
    fn crop_when_content_matches_extended_bounds() {
        // content = DWM bounds size (800x600) -> origin is extended.left/top
        let c = crop_for(&geo(), 800, 600);
        assert_eq!(
            c,
            Crop {
                x: 0,
                y: 30,
                width: 800,
                height: 570
            }
        );
    }

    #[test]
    fn crop_is_clamped_to_content() {
        // content smaller than expected: crop must never exceed content bounds
        let c = crop_for(&geo(), 400, 300);
        assert_eq!(c.x + c.width, 400);
        assert_eq!(c.y + c.height, 300);
    }

    #[test]
    fn crop_never_zero_sized() {
        let mut g = geo();
        g.client_size = (0, 0);
        let c = crop_for(&g, 814, 607);
        assert!(c.width >= 1 && c.height >= 1);
    }

    #[test]
    fn clamp_client_crop_with_monitor_origin() {
        // Window on a second monitor at (1920, 0), 1920x1080: client at (1950, 130), 1900x1000.
        // The crop is relative to the monitor origin and clamped to the monitor's content.
        let g = Geometry {
            window: (1943, 100, 3857, 1207),
            extended: (1950, 100, 3850, 1200),
            client_origin: (1950, 130),
            client_size: (1900, 1000),
        };
        let c = clamp_client_crop(&g, (1920, 0), 1920, 1080);
        assert_eq!(
            c,
            Crop {
                x: 30,
                y: 130,
                width: 1890,
                height: 950
            }
        );
    }
}
