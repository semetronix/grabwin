mod capture;
mod d3d;
mod error;
mod frame;
mod pngenc;
mod window;

use std::sync::Mutex;

use numpy::{IntoPyArray, PyArray3, PyArrayMethods};
use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use capture::{Capture, Mode, Options};
use frame::PixelFormat;
use window::Selector;

pub use error::{Error, Result};
pub use window::WindowInfo;

#[pyfunction]
fn list_windows() -> Vec<WindowInfo> {
    window::list_windows()
}

create_exception!(screenshot_helper, CaptureError, PyException);
create_exception!(screenshot_helper, WindowNotFoundError, CaptureError);
create_exception!(screenshot_helper, WindowClosedError, CaptureError);
create_exception!(screenshot_helper, WindowMinimizedError, CaptureError);
create_exception!(screenshot_helper, CaptureTimeoutError, CaptureError);
create_exception!(screenshot_helper, CaptureUnsupportedError, CaptureError);

impl From<Error> for PyErr {
    fn from(e: Error) -> PyErr {
        let msg = e.to_string();
        match e {
            Error::NotFound(_) => WindowNotFoundError::new_err(msg),
            Error::Closed => WindowClosedError::new_err(msg),
            Error::Minimized => WindowMinimizedError::new_err(msg),
            Error::Timeout(_) => CaptureTimeoutError::new_err(msg),
            Error::Unsupported(_) => CaptureUnsupportedError::new_err(msg),
            Error::Invalid(_) => pyo3::exceptions::PyValueError::new_err(msg),
            Error::Windows(_) | Error::Encode(_) => CaptureError::new_err(msg),
        }
    }
}

fn selector_from_args(
    hwnd: Option<isize>,
    title: Option<String>,
    process: Option<String>,
) -> PyResult<Selector> {
    match (hwnd, title, process) {
        (Some(h), None, None) => Ok(Selector::Hwnd(h)),
        (None, Some(t), None) => Ok(Selector::Title(t)),
        (None, None, Some(p)) => Ok(Selector::Process(p)),
        _ => Err(pyo3::exceptions::PyValueError::new_err(
            "exactly one of hwnd, title or process must be given",
        )),
    }
}

#[pyclass(module = "screenshot_helper")]
pub struct WindowCapture {
    inner: Mutex<Option<Capture>>,
    hwnd: isize,
    mode: &'static str,
}

// Every acquisition of `inner` (and, through it, of the capture state) from Python happens under
// `py.detach`: the WGC callback holds the capture state while it runs, and blocking on it with the
// GIL held would stall every other Python thread for the duration of a frame. The callback itself
// never touches Python (see `capture::on_frame`), which is what keeps the dealloc path (`Drop` with
// the GIL held) safe.
impl WindowCapture {
    fn with_capture<T>(&self, f: impl FnOnce(&Capture) -> Result<T>) -> Result<T> {
        let guard = self.inner.lock().unwrap();
        match guard.as_ref() {
            Some(cap) => f(cap),
            None => Err(Error::Closed),
        }
    }

    fn close_inner(&self) {
        if let Some(mut cap) = self.inner.lock().unwrap().take() {
            cap.close();
        }
    }

    /// Grabs the current frame and PNG-encodes it. Callers run this inside `py.detach` (and, for
    /// `save_png`, alongside the subsequent file write) so the GIL is never held across the capture
    /// or the encode.
    fn encode_current_png(&self, compression: u8) -> Result<Vec<u8>> {
        let frame = self.with_capture(|c| c.grab_bgra())?;
        pngenc::encode_png(&frame.bgra, frame.width, frame.height, compression)
    }
}

#[pymethods]
impl WindowCapture {
    #[new]
    #[pyo3(signature = (hwnd=None, title=None, process=None, mode="on_demand", cursor=false, border=false, timeout_ms=250))]
    fn new(
        py: Python<'_>,
        hwnd: Option<isize>,
        title: Option<String>,
        process: Option<String>,
        mode: &str,
        cursor: bool,
        border: bool,
        timeout_ms: u32,
    ) -> PyResult<Self> {
        let sel = selector_from_args(hwnd, title, process)?;
        let (mode_enum, mode_name) = match mode {
            "on_demand" => (Mode::OnDemand, "on_demand"),
            "live" => (Mode::Live, "live"),
            other => {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "mode must be \"on_demand\" or \"live\", got {other:?}"
                )))
            }
        };
        let opts = Options {
            mode: mode_enum,
            timeout_ms,
            cursor,
            border,
        };
        let (cap, hwnd) = py.detach(|| -> Result<(Capture, isize)> {
            let info = window::find_window(&sel)?;
            Ok((Capture::start(info.hwnd, opts)?, info.hwnd))
        })?;
        Ok(Self {
            inner: Mutex::new(Some(cap)),
            hwnd,
            mode: mode_name,
        })
    }

    #[getter]
    fn hwnd(&self) -> isize {
        self.hwnd
    }

    #[getter]
    fn mode(&self) -> &'static str {
        self.mode
    }

    #[getter]
    fn size(&self, py: Python<'_>) -> PyResult<(u32, u32)> {
        Ok(py.detach(|| self.with_capture(|c| Ok(c.size())))?)
    }

    #[getter]
    fn is_alive(&self, py: Python<'_>) -> bool {
        py.detach(|| {
            let guard = self.inner.lock().unwrap();
            matches!(guard.as_ref(), Some(c) if !c.is_closed()) && window::is_alive(self.hwnd)
        })
    }

    /// Returns (bgra_bytes, width, height).
    fn grab_raw<'py>(&self, py: Python<'py>) -> PyResult<(Bound<'py, PyBytes>, u32, u32)> {
        let frame = py.detach(|| self.with_capture(|c| c.grab_bgra()))?;
        Ok((PyBytes::new(py, &frame.bgra), frame.width, frame.height))
    }

    /// numpy array (h, w, c), uint8. format: "bgra" (default), "rgba", "rgb", "bgr".
    #[pyo3(signature = (format = "bgra"))]
    fn grab<'py>(&self, py: Python<'py>, format: &str) -> PyResult<Bound<'py, PyArray3<u8>>> {
        let fmt = PixelFormat::parse(format)?;
        let (data, w, h) = py.detach(|| -> Result<(Vec<u8>, u32, u32)> {
            let frame = self.with_capture(|c| c.grab_bgra())?;
            Ok((frame.to_format(fmt), frame.width, frame.height))
        })?;
        let arr = data
            .into_pyarray(py)
            .reshape([h as usize, w as usize, fmt.channels()])?;
        Ok(arr)
    }

    /// PNG bytes (RGB, alpha dropped). compression 0..=9, default 1 (fast).
    #[pyo3(signature = (compression = 1))]
    fn grab_png<'py>(&self, py: Python<'py>, compression: u8) -> PyResult<Bound<'py, PyBytes>> {
        let data = py.detach(|| self.encode_current_png(compression))?;
        Ok(PyBytes::new(py, &data))
    }

    #[pyo3(signature = (path, compression = 1))]
    fn save_png(&self, py: Python<'_>, path: std::path::PathBuf, compression: u8) -> PyResult<()> {
        py.detach(|| -> PyResult<()> {
            let data = self.encode_current_png(compression)?;
            std::fs::write(&path, data)?;
            Ok(())
        })
    }

    fn close(&self, py: Python<'_>) {
        py.detach(|| self.close_inner());
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&self, py: Python<'_>, _args: &Bound<'_, pyo3::types::PyTuple>) -> bool {
        py.detach(|| self.close_inner());
        false
    }
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let _ = pyo3_log::try_init();
    let py = m.py();
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("CaptureError", py.get_type::<CaptureError>())?;
    m.add("WindowNotFoundError", py.get_type::<WindowNotFoundError>())?;
    m.add("WindowClosedError", py.get_type::<WindowClosedError>())?;
    m.add(
        "WindowMinimizedError",
        py.get_type::<WindowMinimizedError>(),
    )?;
    m.add("CaptureTimeoutError", py.get_type::<CaptureTimeoutError>())?;
    m.add(
        "CaptureUnsupportedError",
        py.get_type::<CaptureUnsupportedError>(),
    )?;
    m.add_class::<WindowInfo>()?;
    m.add_class::<WindowCapture>()?;
    m.add_function(wrap_pyfunction!(list_windows, m)?)?;
    Ok(())
}
