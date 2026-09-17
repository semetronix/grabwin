import os
import sys

import pytest

import screenshot_helper as sh

pytestmark = pytest.mark.gui


def test_list_windows_finds_tk(tk_window):
    info = tk_window.find()
    assert info is not None
    assert info.hwnd == tk_window.hwnd
    assert info.process.lower() == os.path.basename(sys.executable).lower()
    assert info.pid > 0  # not compared to proc.pid: the venv launcher may spawn the real python.exe as a child
    assert (info.width, info.height) == (640, 480)
    assert info.is_minimized is False
    assert "WindowInfo(" in repr(info)


def test_list_windows_has_no_empty_titles():
    assert all(w.title for w in sh.list_windows())
