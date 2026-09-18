import grabwin as gw


def test_version_is_string():
    assert isinstance(gw.__version__, str) and gw.__version__


def test_exception_hierarchy():
    for name in (
        "WindowNotFoundError",
        "WindowClosedError",
        "WindowMinimizedError",
        "CaptureTimeoutError",
        "CaptureUnsupportedError",
    ):
        exc = getattr(gw, name)
        assert issubclass(exc, gw.CaptureError)
        assert issubclass(exc, Exception)
