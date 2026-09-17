mod error;
mod frame;

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;

pub use error::{Error, Result};

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
    Ok(())
}
