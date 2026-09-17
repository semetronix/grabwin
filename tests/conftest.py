import os
import subprocess
import sys
import time
import uuid
from pathlib import Path

import pytest

import screenshot_helper as sh

HERE = Path(__file__).parent

# Make the test process per-monitor DPI aware so GetSystemMetrics/ctypes coordinates in tests are
# physical pixels, consistent with list_windows() and the DPI-aware Tk child.
try:
    import ctypes

    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:  # already set or unavailable
    pass


def pytest_collection_modifyitems(config, items):
    if os.environ.get("SH_NO_GUI"):
        skip = pytest.mark.skip(reason="SH_NO_GUI set")
        for item in items:
            if "gui" in item.keywords:
                item.add_marker(skip)


def wait_for(pred, timeout=10.0, interval=0.05):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = pred()
        if value:
            return value
        time.sleep(interval)
    raise TimeoutError("condition not met")


class TkWin:
    def __init__(self, proc, title):
        self.proc = proc
        self.title = title
        self.info = wait_for(self.find)
        self.hwnd = self.info.hwnd
        time.sleep(0.3)  # let Tk paint the first frame

    def find(self):
        return next((w for w in sh.list_windows() if w.title == self.title), None)

    def send(self, cmd: str, settle: float = 0.4):
        self.proc.stdin.write(cmd + "\n")
        self.proc.stdin.flush()
        time.sleep(settle)

    def close(self):
        if self.proc.poll() is None:
            try:
                self.send("quit", settle=0.0)
                self.proc.wait(timeout=3)
            except Exception:
                self.proc.kill()


@pytest.fixture
def tk_window():
    title = f"sh-test-{uuid.uuid4().hex[:8]}"
    proc = subprocess.Popen(
        [sys.executable, str(HERE / "tkwin.py"), title, "640", "480"],
        stdin=subprocess.PIPE,
        text=True,
    )
    win = TkWin(proc, title)
    yield win
    win.close()
