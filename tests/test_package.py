import screenshot_helper as sh


def test_version_is_string():
    assert isinstance(sh.__version__, str) and sh.__version__


def test_exception_hierarchy():
    for name in (
        "WindowNotFoundError",
        "WindowClosedError",
        "WindowMinimizedError",
        "CaptureTimeoutError",
        "CaptureUnsupportedError",
    ):
        exc = getattr(sh, name)
        assert issubclass(exc, sh.CaptureError)
        assert issubclass(exc, Exception)
