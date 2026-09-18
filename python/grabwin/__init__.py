"""Fast per-window screenshots on Windows via Windows Graphics Capture."""

from ._native import (  # noqa: F401
    CaptureError,
    CaptureTimeoutError,
    CaptureUnsupportedError,
    WindowCapture,
    WindowClosedError,
    WindowInfo,
    WindowMinimizedError,
    WindowNotFoundError,
    __version__,
    list_windows,
)

__all__ = [
    "CaptureError",
    "CaptureTimeoutError",
    "CaptureUnsupportedError",
    "WindowCapture",
    "WindowClosedError",
    "WindowInfo",
    "WindowMinimizedError",
    "WindowNotFoundError",
    "__version__",
    "list_windows",
]
