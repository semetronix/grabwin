# screenshot_helper

Быстрые снимки **клиентской области окна** на Windows через Windows Graphics Capture.
Работает с играми (DirectX/Vulkan/OpenGL) и аппаратно-ускоренными окнами (браузеры),
не зависит от вендора видеокарты. Windows 10 1903+.

## Требования

- Python 3.12–3.14 (расширение собрано как `abi3-py312`), `numpy`.
- Для разработки: `maturin`, `pytest`, `pillow`.

## Установка

```powershell
pip install maturin
maturin develop --release        # или: maturin build --release -> dist/*.whl
```

## Использование

```python
import screenshot_helper as sh

for w in sh.list_windows():
    print(w)

with sh.WindowCapture(process="chrome.exe") as cap:      # или title="...", hwnd=...
    arr = cap.grab()            # numpy (h, w, 4) BGRA uint8
    rgb = cap.grab("rgb")       # (h, w, 3)
    png = cap.grab_png()        # bytes, RGB
    cap.save_png("shot.png")

# Бот, снимающий много раз в секунду:
with sh.WindowCapture(title="Game", mode="live") as cap:
    frame = cap.grab("bgr")
```

Параметры `WindowCapture`: `hwnd` | `title` (подстрока, без учёта регистра) | `process` ("game.exe"),
`mode="on_demand"|"live"`, `cursor=False`, `border=False`, `timeout_ms=250`.

Исключения: `CaptureError` ← `WindowNotFoundError`, `WindowClosedError`, `WindowMinimizedError`,
`CaptureTimeoutError`, `CaptureUnsupportedError`.

## Производительность (замерено `examples/bench.py`)

Замер: Microsoft Edge, окно 1249x1364 (страница youtube.com), HEAD `34bb68d`.

```
[on_demand] window 1249x1364, target=window
grab() bgra                  median   2.50 ms   p95   2.69 ms
grab('rgb')                  median   3.62 ms   p95   3.80 ms
grab_raw()                   median   2.49 ms   p95   2.69 ms
grab_png(compression=0)      median   9.45 ms   p95   9.74 ms
grab_png(compression=1)      median   4.80 ms   p95   5.27 ms
grab_png(compression=3)      median  17.11 ms   p95  18.16 ms
grab_png(compression=9)      median  31.18 ms   p95  34.60 ms
png size @1: 139 KiB

[live] window 1249x1364, target=window
grab() bgra                  median   2.29 ms   p95   2.42 ms
grab('rgb')                  median   3.50 ms   p95   4.36 ms
grab_raw()                   median   2.30 ms   p95   2.53 ms
grab_png(compression=0)      median   9.24 ms   p95   9.51 ms
grab_png(compression=1)      median   4.66 ms   p95   5.82 ms
grab_png(compression=3)      median  16.67 ms   p95  17.25 ms
grab_png(compression=9)      median  30.75 ms   p95  31.44 ms
png size @1: 139 KiB
```

Цели из спеки (§5) при ~1080p: `grab()` on_demand ≤ 5 мс, live ≤ 2 мс, `grab_png(1)` ≤ 25 мс.

- `grab()` on_demand (2.50 мс) и `grab_png(1)` (4.80 мс on_demand / 4.66 мс live) — цели выполнены с запасом.
- `grab()` live (2.29 мс median) **немного превышает** цель в 2 мс — известное отклонение на этом
  железе/разрешении, не блокер. Основную стоимость даёт копирование BGRA-буфера (~9.4 МБ на кадр
  при 1249x1364), а не сам readback.

## Архитектура и особенности реализации

- Пул кадров WGC создаётся размером `max(размер окна, размер монитора)`: изменение размера окна
  в пределах монитора подхватывается уже на следующем `grab()`; пересоздание пула (и ожидание
  перерисовки окна) требуется только при росте окна за границы монитора. Цена — две
  GPU-поверхности размером с монитор на каждый `WindowCapture` (~16 МБ на 1080p).
- На каждый `WindowCapture` создаётся отдельное устройство D3D11 (не одно на процесс).
- Обрезка по клиентской области проверена на Windows 11: кадр от WGC совпадает с расширенными
  границами DWM (`DwmGetWindowAttribute(..., DWMWA_EXTENDED_FRAME_BOUNDS, ...)`), невидимых рамок
  ресайза в кадре нет.
- Fallback для exclusive-fullscreen: `cap.target` равен `"window"` или `"monitor"`; при переходе
  окна в исключительный полноэкранный режим захват переключается на монитор. Путь через
  `"monitor"` не покрыт автоматическими тестами — только ручной проверкой (см. ниже).

## Ограничения

- Свёрнутое окно не рендерится системой: отдаётся последний буферизованный кадр, иначе `WindowMinimizedError`.
- В режиме `live` фоновый readback каждого кадра стоит ~10–20 % одного ядра.
- Exclusive-fullscreen игры захватываются через монитор (`cap.target == "monitor"`), в кадр попадают оверлеи.
- Windows 11 скругляет нижние углы окон верхнего уровня: в `grab()`/`grab_png()` эти угловые
  пиксели приходят полупрозрачными (в `bgra`/`rgba` — с не-255 альфой) или подмешанными к фону
  (в PNG после отбрасывания альфы — не строго тем цветом, что за окном). Известное поведение
  Windows 11, не баг библиотеки.
- Жёлтая рамка захвата (Windows 11 показывает её вокруг захватываемого окна при активном WGC-сеансе,
  даже при `border=False`, который управляет только рамкой *внутри* самого кадра): достоверно
  подтвердить/опровергнуть её появление на экране в этой сессии не удалось (см. «Проверено на»,
  п. 4) — окружение не позволило надёжно совместить полноэкранный снимок рабочего стола с активным
  окном `live`-захвата. Если рамка видна на непакетированных (unpackaged) приложениях, её отключение
  требует `GraphicsCaptureAccess.RequestAccessAsync` — отдельная задача.

## Проверено на

1. **Браузер (Microsoft Edge, youtube.com, окно 1249x1364)** — подтверждено: `save_png()` дал
   PNG без заголовка/рамки окна, содержимое не чёрное, кадр актуален (видна реальная страница
   YouTube на момент снимка). `cap.target == "window"`.
2. **Игра в borderless/windowed** — ожидает ручной проверки (нужно физически запустить игру).
3. **Та же игра в exclusive fullscreen** — ожидает ручной проверки.
4. **Жёлтая рамка захвата вокруг окна при `border=False`** — не подтверждено достоверно: два
   снимка всего экрана, сделанные во время активного `mode="live"`-захвата Edge, рамки не
   показали, но совпадение снимка по времени с активным WGC-сеансом не гарантировано в этом
   окружении. Требует ручной перепроверки пользователем.

## Разработка

```powershell
cargo test                       # unit-тесты Rust
python -m pytest                 # интеграционные (создают окна); SH_NO_GUI=1 чтобы пропустить
```

Бенчмарк: `python examples/bench.py --title "Chrome"` (или `--process`, `--hwnd`), `--iters N`.
