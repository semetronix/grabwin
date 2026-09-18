# screenshot_helper

[English](README.md) | **Русский**

Быстрые снимки **клиентской области окна** на Windows через Windows Graphics Capture.
Работает с играми (DirectX/Vulkan/OpenGL) и аппаратно-ускоренными окнами (браузеры),
не зависит от вендора видеокарты. Windows 10 1903+.

Кадр отдаётся как массив `numpy` или как PNG в `bytes` за единицы миллисекунд.

## Требования

- Windows 10 1903 (сборка 18362) или новее.
- Python 3.12–3.14 (расширение собрано как `abi3-py312`), `numpy`.
- Для разработки: Rust toolchain (stable, MSVC), `maturin`, `pytest`, `pillow`.

## Установка

Из исходников (нужен Rust):

```powershell
pip install maturin
maturin develop --release        # или: maturin build --release -> dist/*.whl
```

## Использование

```python
import screenshot_helper as sh

# Список окон, доступных для захвата (видимые, с заголовком, верхнего уровня)
for w in sh.list_windows():
    print(w)          # WindowInfo(hwnd=..., title=..., process="chrome.exe", pid=..., width=..., height=..., is_minimized=False)

# Захват по имени процесса (или title="..." / hwnd=...)
with sh.WindowCapture(process="chrome.exe") as cap:
    arr = cap.grab()            # numpy uint8, форма (h, w, 4), BGRA
    rgb = cap.grab("rgb")       # форма (h, w, 3)
    png = cap.grab_png()        # bytes, RGB PNG
    cap.save_png("shot.png")
    data, w, h = cap.grab_raw() # (bytes BGRA, ширина, высота) — без numpy

# Бот, снимающий много раз в секунду:
with sh.WindowCapture(title="Game", mode="live") as cap:
    while True:
        frame = cap.grab("bgr")   # ~1 мс
        ...
```

### Параметры `WindowCapture(...)`

| Параметр | По умолчанию | Смысл |
|---|---|---|
| `hwnd` | `None` | Дескриптор окна. Нужно указать ровно один из `hwnd` / `title` / `process`. |
| `title` | `None` | Подстрока заголовка без учёта регистра; берётся первое видимое окно верхнего уровня. |
| `process` | `None` | Имя исполняемого файла, например `"game.exe"` (без учёта регистра); самое большое видимое окно этого процесса. |
| `mode` | `"on_demand"` | `"on_demand"` — кадр читается с GPU только при вызове `grab*()` (в простое нагрузка почти нулевая). `"live"` — каждый кадр читается в фоне, `grab()` — просто копия памяти (~1 мс), цена ~10–20 % одного ядра CPU. |
| `cursor` | `False` | Рисовать курсор мыши в кадре. |
| `border` | `False` | Оставлять рамку захвата внутри кадра. |
| `timeout_ms` | `250` | Сколько `grab*()` ждёт первый кадр. |

### Методы и свойства

- `grab(format="bgra")` → `numpy.ndarray`, `uint8`, C-contiguous. Форматы: `"bgra"`, `"rgba"` (форма `(h, w, 4)`), `"rgb"`, `"bgr"` (форма `(h, w, 3)`). Массив владеет своим буфером — его можно хранить сколько угодно.
- `grab_png(compression=1)` → `bytes`. RGB PNG, альфа отбрасывается. `0` — без сжатия (самый быстрый, самый большой), `1` — быстрый (по умолчанию), `2–5` — сбалансированный, `6–9` — максимальный.
- `save_png(path, compression=1)` — `grab_png()` с записью в файл.
- `grab_raw()` → `(bytes, width, height)` — плотно упакованный BGRA.
- `size` → `(width, height)` клиентской области (обновляется после ресайза).
- `hwnd`, `mode`, `target` (`"window"` или `"monitor"`, см. ниже), `is_alive`.
- `close()` — остановить сессию захвата; повторный вызов безопасен. Поддерживается `with ...`.

Все `grab*()` отпускают GIL на время ожидания, чтения с GPU и кодирования — другие потоки Python не блокируются.

### Исключения

Все наследуют `screenshot_helper.CaptureError`:

| Исключение | Когда |
|---|---|
| `WindowNotFoundError` | Селектор не нашёл окно. |
| `WindowClosedError` | Окно закрыто (или вызван `close()`). |
| `WindowMinimizedError` | Окно свёрнуто, а буферизованного кадра ещё нет. |
| `CaptureTimeoutError` | Кадр не пришёл за `timeout_ms`. |
| `CaptureUnsupportedError` | Windows Graphics Capture недоступен (Windows < 10 1903). |

`ValueError` — при неверных аргументах (неизвестный формат, два селектора сразу, `compression > 9`).

### Логирование

Rust-ядро пишет в стандартный `logging` (логгер `screenshot_helper`, уровень `DEBUG` — детали сессии: вычисленная обрезка, переключение на монитор). Настраивайте `logging` **до** первого вызова захвата — уровни кешируются при первом обращении.

## Производительность

Замерено `examples/bench.py` на Microsoft Edge (youtube.com), окно 1249×1364:

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

- `grab()` для `bgra`/`rgba` отдаёт буфер readback'а в numpy без второй копии (`rgba` — перестановка каналов на месте). `rgb`/`bgr` выделяют трёхканальную копию.
- `grab_raw()` стоит одну лишнюю копию: `bytes` не может забрать готовый буфер.

Запустить самому: `python examples/bench.py --title "Chrome"` (или `--process chrome.exe`, `--hwnd N`), `--iters N`.

## Как это работает

- Окно захватывается композитором в GPU-текстуру (Windows Graphics Capture). Каждый кадр обрезается по клиентской области на GPU; дорогое чтение GPU→CPU выполняется только когда нужно (`on_demand`) или в фоне (`live`).
- Пул кадров создаётся размером `max(окно, монитор)`, поэтому ресайз в пределах монитора виден уже на следующем `grab()`; только рост окна за границы монитора пересоздаёт пул и ждёт перерисовки окна. Цена — две GPU-поверхности размером с монитор на каждый `WindowCapture` (~16 МБ на 1080p).
- Каждый `WindowCapture` владеет собственным устройством D3D11.
- Обрезка по клиентской области проверена на Windows 11: кадр совпадает с расширенными границами DWM (невидимых рамок ресайза в кадре нет).
- Размер клиентской области измеряется в физических пикселях, поэтому DPI-unaware окна на масштабированных мониторах (125–150 %) обрезаются правильно.
- Fallback для exclusive-fullscreen: если окно размером с монитор не отдаёт кадров, захват переключается на монитор; `cap.target` равен `"window"` или `"monitor"`.

## Ограничения и известное поведение

- Свёрнутое окно не рендерится системой: отдаётся последний буферизованный кадр, иначе `WindowMinimizedError`.
- В режиме `live` каждый кадр читается в фоне — ~10–20 % одного ядра CPU.
- Exclusive-fullscreen игры захватываются через монитор (`cap.target == "monitor"`); оверлеи поверх игры попадают в кадр.
- Windows 11 скругляет нижние углы окон верхнего уровня: эти угловые пиксели приходят полупрозрачными (`bgra`/`rgba`, альфа < 255) или подмешанными (PNG). Это поведение Windows, не баг.
- Windows 11 может показывать жёлтую рамку вокруг захватываемого окна у непакетированных приложений независимо от `border=False` (он управляет только рамкой *внутри* кадра). Достоверно не проверено; если рамка видна, для её отключения нужен `GraphicsCaptureAccess.RequestAccessAsync` — запланировано.
- `title`/`process` разрешаются один раз в конструкторе. Если целевое приложение перезапустилось — создайте новый `WindowCapture`.

## Проверено на

1. **Браузер (Microsoft Edge, youtube.com, 1249×1364)** — подтверждено: `save_png()` дал PNG без заголовка/рамки, содержимое не чёрное, кадр актуален. `cap.target == "window"`.
2. **Игра в borderless/windowed** — ожидает ручной проверки.
3. **Та же игра в exclusive fullscreen** — ожидает ручной проверки.
4. **Монитор с масштабом 125–150 %** — ожидает ручной проверки (код покрыт тестом, который на 100 % проходит тривиально).
5. **Жёлтая рамка захвата при `border=False`** — достоверно не проверено.

## Разработка

```powershell
cargo test                       # unit-тесты Rust
python -m pytest                 # интеграционные (открывают реальные окна); SH_NO_GUI=1 чтобы пропустить
```

Проектные заметки — в `docs/superpowers/specs/`.
