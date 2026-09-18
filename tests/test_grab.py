import io

import numpy as np
import pytest
from PIL import Image

import grabwin as gw

pytestmark = pytest.mark.gui


def test_grab_bgra_default(tk_window):
    with gw.WindowCapture(hwnd=tk_window.hwnd) as cap:
        arr = cap.grab()
        assert arr.dtype == np.uint8 and arr.shape == (480, 640, 4)
        assert arr.flags["C_CONTIGUOUS"]
        px = arr[240, 320]
        assert px[0] < 10 and px[1] < 10 and px[2] > 245  # B G R


@pytest.mark.parametrize("fmt,channels,red", [("rgb", 3, 0), ("bgr", 3, 2), ("rgba", 4, 0), ("BGRA", 4, 2)])
def test_grab_formats(tk_window, fmt, channels, red):
    with gw.WindowCapture(hwnd=tk_window.hwnd) as cap:
        arr = cap.grab(fmt)
        assert arr.shape == (480, 640, channels)
        assert arr[0, 0, red] > 245
        assert all(arr[0, 0, c] < 10 for c in range(3) if c != red)


def test_grab_bad_format(tk_window):
    with gw.WindowCapture(hwnd=tk_window.hwnd) as cap:
        with pytest.raises(ValueError):
            cap.grab("yuv")


@pytest.mark.parametrize("level", [0, 1, 9])
def test_grab_png(tk_window, level):
    with gw.WindowCapture(hwnd=tk_window.hwnd) as cap:
        data = cap.grab_png(compression=level)
        assert isinstance(data, bytes) and data[:8] == b"\x89PNG\r\n\x1a\n"
        img = Image.open(io.BytesIO(data))
        assert img.size == (640, 480) and img.mode == "RGB"
        assert img.getpixel((320, 240))[0] > 245


def test_save_png(tk_window, tmp_path):
    out = tmp_path / "shot.png"
    with gw.WindowCapture(hwnd=tk_window.hwnd) as cap:
        cap.save_png(out)
    assert Image.open(out).size == (640, 480)


def test_grab_array_owned_by_caller(tk_window):
    with gw.WindowCapture(hwnd=tk_window.hwnd) as cap:
        a = cap.grab()
        b = cap.grab()
        a[:] = 0
        assert b[240, 320, 2] > 245  # b is an independent buffer
