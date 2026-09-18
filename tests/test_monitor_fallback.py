import pytest

import screenshot_helper as sh

pytestmark = pytest.mark.gui


def test_normal_window_uses_window_target(tk_window):
    with sh.WindowCapture(hwnd=tk_window.hwnd) as cap:
        assert cap.target == "window"


def test_fullscreen_sized_window_still_captures(tk_window):
    # Make the Tk window cover the whole primary monitor; WGC still captures it as a window,
    # so target stays "window" — this verifies the fullscreen check does not misfire.
    import ctypes

    user32 = ctypes.windll.user32
    w, h = user32.GetSystemMetrics(0), user32.GetSystemMetrics(1)
    tk_window.send(f"geometry {w}x{h}+0+0", settle=1.0)
    with sh.WindowCapture(hwnd=tk_window.hwnd) as cap:
        arr = cap.grab()
        assert arr.shape[0] >= h - 100 and arr.shape[1] >= w - 100
        assert cap.target == "window"
