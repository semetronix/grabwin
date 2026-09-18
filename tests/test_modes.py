import time

import pytest

import grabwin as gw

pytestmark = pytest.mark.gui


@pytest.mark.parametrize("mode", ["on_demand", "live"])
def test_modes_grab(tk_window, mode):
    with gw.WindowCapture(hwnd=tk_window.hwnd, mode=mode, cursor=False, border=False) as cap:
        assert cap.mode == mode
        for _ in range(5):
            arr = cap.grab()
            assert arr.shape == (480, 640, 4)
            assert arr[240, 320, 2] > 245


def test_bad_mode(tk_window):
    with pytest.raises(ValueError):
        gw.WindowCapture(hwnd=tk_window.hwnd, mode="turbo")


def test_static_window_does_not_time_out(tk_window):
    # Tk window is static; WGC sends no new frames, yet grab() must keep returning the buffered one.
    with gw.WindowCapture(hwnd=tk_window.hwnd, timeout_ms=100) as cap:
        cap.grab()
        time.sleep(1.0)
        t0 = time.perf_counter()
        cap.grab()
        assert time.perf_counter() - t0 < 0.05


def test_live_mode_is_fast(tk_window):
    with gw.WindowCapture(hwnd=tk_window.hwnd, mode="live") as cap:
        cap.grab()
        t0 = time.perf_counter()
        for _ in range(20):
            cap.grab()
        per_call = (time.perf_counter() - t0) / 20
        assert per_call < 0.01, per_call
