"""Shared pytest fixtures for the Taro SDK suite (local-only; no CI)."""

import os

import pytest


@pytest.fixture
def server_url():
    """Base URL of a live Taro server for integration tests.

    Integration tests are opt-in: start the server, then run with
    `TARO_TEST_SERVER_URL=http://localhost:8080 pytest -m integration`.
    Absent the env var, the test is skipped rather than failed.
    """
    url = os.environ.get("TARO_TEST_SERVER_URL")
    if not url:
        pytest.skip("set TARO_TEST_SERVER_URL to run integration tests")
    return url
