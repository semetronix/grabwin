"""Latency benchmark: python examples/bench.py --title "Chrome" [--iters 200]"""
import argparse
import statistics
import time

import grabwin as gw


def bench(label, fn, iters):
    fn()  # warm-up
    times = []
    for _ in range(iters):
        t0 = time.perf_counter()
        fn()
        times.append((time.perf_counter() - t0) * 1000)
    times.sort()
    print(f"{label:<28} median {statistics.median(times):6.2f} ms   p95 {times[int(len(times) * 0.95)]:6.2f} ms")


def main():
    ap = argparse.ArgumentParser()
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument("--title")
    g.add_argument("--process")
    g.add_argument("--hwnd", type=int)
    ap.add_argument("--iters", type=int, default=200)
    a = ap.parse_args()
    sel = {k: v for k, v in (("title", a.title), ("process", a.process), ("hwnd", a.hwnd)) if v is not None}

    for mode in ("on_demand", "live"):
        with gw.WindowCapture(mode=mode, **sel) as cap:
            w, h = cap.size
            print(f"\n[{mode}] window {w}x{h}, target={cap.target}")
            bench("grab() bgra", cap.grab, a.iters)
            bench("grab('rgb')", lambda: cap.grab("rgb"), a.iters)
            bench("grab_raw()", cap.grab_raw, a.iters)
            for level in (0, 1, 3, 9):
                bench(f"grab_png(compression={level})", lambda: cap.grab_png(level), max(20, a.iters // 4))
            print(f"png size @1: {len(cap.grab_png(1)) / 1024:.0f} KiB")


if __name__ == "__main__":
    main()
