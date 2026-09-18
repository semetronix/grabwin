import time

import pytest

import grabwin as gw

pytestmark = pytest.mark.gui


@pytest.mark.parametrize("mode", ["on_demand", "live"])
def test_resize_updates_frame_size(tk_window, mode):
    with gw.WindowCapture(hwnd=tk_window.hwnd, mode=mode) as cap:
        assert cap.grab().shape[:2] == (480, 640)
        tk_window.send("geometry 800x600", settle=0.8)
        arr = cap.grab()
        assert arr.shape[:2] == (600, 800)
        assert cap.size == (800, 600)
        assert arr[300, 400, 2] > 245


def test_minimize_returns_last_frame_then_recovers(tk_window):
    with gw.WindowCapture(hwnd=tk_window.hwnd, timeout_ms=200) as cap:
        cap.grab()
        tk_window.send("iconify", settle=0.8)
        t0 = time.perf_counter()
        arr = cap.grab()  # buffered frame, must not block until timeout
        assert time.perf_counter() - t0 < 0.15
        assert arr.shape[:2] == (480, 640)
        tk_window.send("deiconify", settle=0.8)
        tk_window.send("geometry 700x500", settle=0.8)
        arr = cap.grab()
        assert arr.shape[:2] == (500, 700)
        assert arr[250, 350, 2] > 245


def test_minimized_before_first_frame_raises(tk_window):
    tk_window.send("iconify", settle=0.8)
    try:
        with gw.WindowCapture(hwnd=tk_window.hwnd, timeout_ms=200) as cap:
            with pytest.raises(gw.WindowMinimizedError):
                cap.grab()
    finally:
        tk_window.send("deiconify", settle=0.3)


def test_window_closed_raises(tk_window):
    cap = gw.WindowCapture(hwnd=tk_window.hwnd, timeout_ms=200)
    cap.grab()
    tk_window.send("quit", settle=0.8)
    assert cap.is_alive is False
    with pytest.raises(gw.WindowClosedError):
        cap.grab()
    cap.close()


def test_close_is_idempotent_and_grab_after_close_raises(tk_window):
    cap = gw.WindowCapture(hwnd=tk_window.hwnd)
    cap.close()
    cap.close()
    with pytest.raises(gw.WindowClosedError):
        cap.grab()
