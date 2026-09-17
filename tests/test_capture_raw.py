import os
import sys

import pytest

import screenshot_helper as sh

pytestmark = pytest.mark.gui


def test_grab_raw_dimensions_and_colour(tk_window):
    with sh.WindowCapture(hwnd=tk_window.hwnd) as cap:
        assert cap.hwnd == tk_window.hwnd
        data, w, h = cap.grab_raw()
        assert (w, h) == (640, 480)
        assert cap.size == (640, 480)
        assert isinstance(data, bytes) and len(data) == w * h * 4
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
        non_red = sum(1 for r in data[2::4] if r <= 245)
        assert non_red < 512, non_red


def test_selectors_resolve_same_window(tk_window):
    with sh.WindowCapture(title=tk_window.title.upper()) as by_title:
        assert by_title.hwnd == tk_window.hwnd
    # process selector picks the largest visible window of that process; other python.exe windows
    # may exist on the machine, so only check it lands on *some* python.exe window.
    proc_name = os.path.basename(sys.executable)
    with sh.WindowCapture(process=proc_name.upper()) as by_proc:
        python_hwnds = {w.hwnd for w in sh.list_windows() if w.process.lower() == proc_name.lower()}
        assert by_proc.hwnd in python_hwnds


def test_not_found():
    with pytest.raises(sh.WindowNotFoundError):
        sh.WindowCapture(title="sh-no-such-window-8c1f")


def test_selector_validation():
    with pytest.raises(ValueError):
        sh.WindowCapture()
    with pytest.raises(ValueError):
        sh.WindowCapture(hwnd=1, title="x")
