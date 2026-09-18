import os
import sys
import threading
import time

import pytest

import grabwin as gw

pytestmark = pytest.mark.gui


def assert_all_red(data: bytes, w: int, h: int) -> None:
    # BGRA: red background -> B=0, G=0, R=255. Every edge is sampled at its midpoint (plus the
    # top corners), so a 1px crop shift in any direction is caught. The bottom corners are NOT
    # sampled: Windows 11 composites top-level windows with rounded corners (~8px at 100% DPI),
    # and WGC delivers those pixels blended towards transparent.
    points = [(0, 0), (w - 1, 0), (w // 2, h // 2), (0, h // 2), (w - 1, h // 2), (w // 2, h - 1)]
    for (x, y) in points:
        i = (y * w + x) * 4
        assert data[i] < 10 and data[i + 1] < 10 and data[i + 2] > 245, (x, y)
    # Everything except the two rounded bottom corners must be red (corner radius <= 16px
    # even at high DPI, so < 2 * 16 * 16 pixels may deviate).
    non_red = sum(
        1
        for b, g, r in zip(data[0::4], data[1::4], data[2::4])
        if not (b < 10 and g < 10 and r > 245)
    )
    assert non_red < 512, non_red


def test_grab_raw_dimensions_and_colour(tk_window):
    with gw.WindowCapture(hwnd=tk_window.hwnd) as cap:
        assert cap.hwnd == tk_window.hwnd
        data, w, h = cap.grab_raw()
        assert (w, h) == (640, 480)
        assert cap.size == (640, 480)
        assert isinstance(data, bytes) and len(data) == w * h * 4
        assert_all_red(data, w, h)


def test_dpi_unaware_window_crop_covers_client_area(tk_window_nodpi):
    """A DPI-unaware window on a 125-150 % monitor is bitmap-stretched by Windows: its
    GetClientRect is in logical pixels while the WGC frame and ClientToScreen are physical. The
    crop must use the physical client size, or it would cover only the top-left part of the
    client area. On a 100 % monitor this passes trivially (logical == physical) and only guards
    the code path; the assertions bite on scaled setups."""
    info = tk_window_nodpi.info
    with gw.WindowCapture(hwnd=tk_window_nodpi.hwnd) as cap:
        data, w, h = cap.grab_raw()
        assert (w, h) == cap.size == (info.width, info.height)
        assert w >= 640 and h >= 480  # physical size is never smaller than the logical request
        assert_all_red(data, w, h)


def test_selectors_resolve_same_window(tk_window):
    with gw.WindowCapture(title=tk_window.title.upper()) as by_title:
        assert by_title.hwnd == tk_window.hwnd
    # process selector picks the largest visible window of that process; other python.exe windows
    # may exist on the machine, so only check it lands on *some* python.exe window.
    proc_name = os.path.basename(sys.executable)
    with gw.WindowCapture(process=proc_name.upper()) as by_proc:
        python_hwnds = {w.hwnd for w in gw.list_windows() if w.process.lower() == proc_name.lower()}
        assert by_proc.hwnd in python_hwnds


def test_not_found():
    with pytest.raises(gw.WindowNotFoundError):
        gw.WindowCapture(title="sh-no-such-window-8c1f")


def test_selector_validation():
    with pytest.raises(ValueError):
        gw.WindowCapture()
    with pytest.raises(ValueError):
        gw.WindowCapture(hwnd=1, title="x")


def test_size_polling_during_resize_does_not_deadlock(tk_window):
    """Regression: the WGC callback used to log (taking the GIL) while holding the capture state,
    while a Python thread holding the GIL blocked on that state in `size` -> deadlock on resize."""
    with gw.WindowCapture(hwnd=tk_window.hwnd, timeout_ms=2000) as cap:
        assert cap.grab_raw()[1:] == (640, 480)
        stop = threading.Event()
        sizes = []

        def spam():
            while not stop.is_set():
                sizes.append(cap.size)

        t = threading.Thread(target=spam, daemon=True)
        t.start()
        try:
            tk_window.send("geometry 800x600")
            deadline = time.monotonic() + 5.0
            while time.monotonic() < deadline:
                data, w, h = cap.grab_raw()
                if (w, h) == (800, 600):
                    break
                time.sleep(0.05)
            assert (w, h) == (800, 600)
            assert len(data) == w * h * 4
            assert_all_red(data, w, h)
        finally:
            stop.set()
            t.join(timeout=5.0)
        assert not t.is_alive()
        assert (800, 600) in sizes
