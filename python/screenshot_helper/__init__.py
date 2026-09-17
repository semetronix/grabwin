"""Fast per-window screenshots on Windows via Windows Graphics Capture."""

from ._native import (  # noqa: F401
    CaptureError,
    CaptureTimeoutError,
    CaptureUnsupportedError,
    WindowClosedError,
    WindowMinimizedError,
    WindowNotFoundError,
    __version__,
)

__all__ = [
    "CaptureError",
    "CaptureTimeoutError",
    "CaptureUnsupportedError",
    "WindowClosedError",
    "WindowMinimizedError",
    "WindowNotFoundError",
    "__version__",
]
