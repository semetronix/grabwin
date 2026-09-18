"""Child process for GUI tests: shows a solid-colour Tk window, obeys stdin commands.

argv: title width height [nodpi]. With "nodpi" the process stays DPI-unaware, so on a scaled
monitor Windows bitmap-stretches the window and its client rect is in logical pixels.
"""
import ctypes
import queue
import sys
import threading
import tkinter as tk

title, width, height = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
if sys.argv[4:5] != ["nodpi"]:
    # Per-monitor DPI aware, so Tk geometry is in physical pixels (matches list_windows()).
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
commands: "queue.Queue[str]" = queue.Queue()

root = tk.Tk()
root.title(title)
root.geometry(f"{width}x{height}+120+120")
root.configure(bg="#ff0000")


def apply(cmd: str) -> None:
    if cmd.startswith("geometry "):
        root.geometry(cmd.split(" ", 1)[1])
    elif cmd == "iconify":
        root.iconify()
    elif cmd == "deiconify":
        root.deiconify()
    elif cmd == "quit":
        root.destroy()


def reader() -> None:
    for line in sys.stdin:
        commands.put(line.strip())


def poll() -> None:
    try:
        while True:
            apply(commands.get_nowait())
    except queue.Empty:
        pass
    except tk.TclError:
        return  # window destroyed
    root.after(20, poll)


threading.Thread(target=reader, daemon=True).start()
root.after(20, poll)
root.mainloop()
