# grabwin

**English** | [Русский](README.ru.md)

Fast screenshots of a **window's client area** on Windows via Windows Graphics Capture.
Works with games (DirectX/Vulkan/OpenGL) and hardware-accelerated windows (browsers),
independent of the GPU vendor. Windows 10 1903+.

Frames come back as a `numpy` array or as PNG `bytes`, in a few milliseconds.

## Requirements

- Windows 10 1903 (build 18362) or newer.
- Python 3.12–3.14 (the extension is built as `abi3-py312`), `numpy`.
- For development: Rust toolchain (stable, MSVC), `maturin`, `pytest`, `pillow`.

## Installation

From source (Rust toolchain required):

```powershell
pip install maturin
maturin develop --release        # or: maturin build --release -> dist/*.whl
```

## Usage

```python
import grabwin as gw

# List capturable windows (visible, titled, top-level)
for w in gw.list_windows():
    print(w)          # WindowInfo(hwnd=..., title=..., process="chrome.exe", pid=..., width=..., height=..., is_minimized=False)

# Capture by process name (or title="..." / hwnd=...)
with gw.WindowCapture(process="chrome.exe") as cap:
    arr = cap.grab()            # numpy uint8, shape (h, w, 4), BGRA
    rgb = cap.grab("rgb")       # shape (h, w, 3)
    png = cap.grab_png()        # bytes, RGB PNG
    cap.save_png("shot.png")
    data, w, h = cap.grab_raw() # (bytes BGRA, width, height) — no numpy needed

# A bot grabbing frames many times per second:
with gw.WindowCapture(title="Game", mode="live") as cap:
    while True:
        frame = cap.grab("bgr")   # ~1 ms
        ...
```

### `WindowCapture(...)` parameters

| Parameter | Default | Meaning |
|---|---|---|
| `hwnd` | `None` | Window handle. Exactly one of `hwnd` / `title` / `process` must be given. |
| `title` | `None` | Case-insensitive substring of the window title; the first visible top-level match wins. |
| `process` | `None` | Executable name, e.g. `"game.exe"` (case-insensitive); the largest visible window of that process. |
| `mode` | `"on_demand"` | `"on_demand"` — the frame is read from the GPU only when you call `grab*()` (near-zero idle cost). `"live"` — every frame is read back in the background, `grab()` is a memory copy (~1 ms) at the cost of ~10–20 % of one CPU core. |
| `cursor` | `False` | Draw the mouse cursor into the frame. |
| `border` | `False` | Keep the capture border inside the frame. |
| `timeout_ms` | `250` | How long `grab*()` waits for the first frame. |

### Methods and properties

- `grab(format="bgra")` → `numpy.ndarray`, `uint8`, C-contiguous. Formats: `"bgra"`, `"rgba"` (shape `(h, w, 4)`), `"rgb"`, `"bgr"` (shape `(h, w, 3)`). The array owns its buffer — keep it as long as you like.
- `grab_png(compression=1)` → `bytes`. RGB PNG, alpha dropped. `0` = stored (fastest, largest), `1` = fast (default), `2–5` = balanced, `6–9` = high.
- `save_png(path, compression=1)` — `grab_png()` written to a file.
- `grab_raw()` → `(bytes, width, height)` — tightly packed BGRA.
- `size` → `(width, height)` of the client area (updates after a resize).
- `hwnd`, `mode`, `target` (`"window"` or `"monitor"`, see below), `is_alive`.
- `close()` — stop the capture session; idempotent. Also a context manager (`with ...`).

All `grab*()` calls release the GIL while waiting, reading back and encoding, so other Python threads keep running.

### Exceptions

All derive from `grabwin.CaptureError`:

| Exception | When |
|---|---|
| `WindowNotFoundError` | The selector matched no window. |
| `WindowClosedError` | The window was closed (or `close()` was called). |
| `WindowMinimizedError` | The window is minimized and no frame is buffered yet. |
| `CaptureTimeoutError` | No frame arrived within `timeout_ms`. |
| `CaptureUnsupportedError` | Windows Graphics Capture is unavailable (Windows < 10 1903). |

`ValueError` is raised for bad arguments (unknown format, two selectors at once, `compression > 9`).

### Logging

The Rust core logs through Python's `logging` module (logger `grabwin`, level `DEBUG` for session details such as the computed crop and fullscreen fallback). Configure `logging` **before** the first capture call — log levels are cached on first use.

## Performance

Measured with `examples/bench.py` on Microsoft Edge (youtube.com), window 1249×1364:

```
[on_demand] window 1249x1364, target=window
grab() bgra                  median   1.29 ms   p95   1.53 ms
grab('rgb')                  median   3.53 ms   p95   3.69 ms
grab_raw()                   median   2.45 ms   p95   2.60 ms
grab_png(compression=0)      median   9.38 ms   p95   9.62 ms
grab_png(compression=1)      median   4.52 ms   p95   4.67 ms
grab_png(compression=3)      median  16.45 ms   p95  20.80 ms
grab_png(compression=9)      median  28.83 ms   p95  31.78 ms
png size @1: 127 KiB

[live] window 1249x1364, target=window
grab() bgra                  median   1.11 ms   p95   1.22 ms
grab('rgb')                  median   3.44 ms   p95   3.62 ms
grab_raw()                   median   2.29 ms   p95   2.40 ms
grab_png(compression=0)      median   9.11 ms   p95   9.42 ms
grab_png(compression=1)      median   4.49 ms   p95   4.99 ms
grab_png(compression=3)      median  16.14 ms   p95  16.47 ms
grab_png(compression=9)      median  28.26 ms   p95  30.18 ms
png size @1: 127 KiB
```

- `grab()` for `bgra`/`rgba` hands the readback buffer to numpy without a second copy (`rgba` swaps channels in place). `rgb`/`bgr` allocate a 3-channel copy.
- `grab_raw()` costs one extra copy: `bytes` cannot take over an existing buffer.

Run it yourself: `python examples/bench.py --title "Chrome"` (or `--process chrome.exe`, `--hwnd N`), `--iters N`.

## How it works

- The window is captured by the compositor into a GPU texture (Windows Graphics Capture). Every frame is cropped to the client area on the GPU; the expensive GPU→CPU readback happens only when needed (`on_demand`) or in the background (`live`).
- The frame pool is sized to `max(window, monitor)`, so a resize within the monitor is reflected on the very next `grab()`; only growing beyond the monitor recreates the pool and waits for the window to repaint. Cost: two monitor-sized GPU surfaces per `WindowCapture` (~16 MB at 1080p).
- Each `WindowCapture` owns its own D3D11 device.
- Client-area cropping was verified on Windows 11: the captured frame equals the DWM extended frame bounds (no invisible resize borders).
- Client size is measured in physical pixels, so DPI-unaware target windows on scaled monitors (125–150 %) are cropped correctly.
- Exclusive-fullscreen fallback: if a monitor-sized window yields no frames, the capture switches to the monitor; `cap.target` is `"window"` or `"monitor"`.

## Limitations and known behaviour

- A minimized window is not rendered by Windows: you get the last buffered frame, or `WindowMinimizedError` if there is none yet.
- `live` mode reads back every frame in the background — ~10–20 % of one CPU core.
- Exclusive-fullscreen games are captured through the monitor (`cap.target == "monitor"`); overlays on top of the game end up in the frame.
- Windows 11 rounds the bottom corners of top-level windows: those corner pixels come back semi-transparent (`bgra`/`rgba` alpha < 255) or blended (PNG). This is Windows behaviour, not a bug.
- Windows 11 may show a yellow capture border around the captured window on unpackaged apps regardless of `border=False` (which only controls the border *inside* the frame). Not reliably verified yet; if it appears, disabling it needs `GraphicsCaptureAccess.RequestAccessAsync` — a planned follow-up.
- `title`/`process` are resolved once in the constructor. If the target restarts, create a new `WindowCapture`.

## Verified on

1. **Browser (Microsoft Edge, youtube.com, 1249×1364)** — confirmed: `save_png()` produced a PNG without title bar/frame, content not black, frame current. `cap.target == "window"`.
2. **A game in borderless/windowed mode** — pending manual check.
3. **The same game in exclusive fullscreen** — pending manual check.
4. **Monitor scaled to 125–150 %** — pending manual check (the code path is covered by a test that passes trivially at 100 %).
5. **Yellow capture border with `border=False`** — not reliably verified.

## Development

```powershell
cargo test                       # Rust unit tests
python -m pytest                 # integration tests (they open real windows); SH_NO_GUI=1 to skip
```

Design notes live in `docs/superpowers/specs/`.

## License

MIT — see [LICENSE](LICENSE).
