"""After each client test, whatever client it left open (`harness.LIVE`): one whose test failed before its `with`."""

import pytest

from tests.base.client import harness


@pytest.fixture(autouse=True)
def close_clients():
    yield
    for c in list(harness.LIVE):
        c.close()
