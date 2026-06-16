"""Thin HTTP client over the Taro REST API (`/api/v1`).

Stdlib-only (urllib) so the POC SDK has zero install footprint. Non-2xx and
transport failures surface as `TaroHTTPError`; callers decide whether to swallow
them (the SDK's never-crash policy) or raise.
"""

import json
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Dict, Optional


class TaroHTTPError(Exception):
    """Any failed request: HTTP status (0 = transport/connection failure)."""

    def __init__(self, status: int, message: str):
        super().__init__(f"HTTP {status}: {message}")
        self.status = status
        self.message = message


class Client:
    def __init__(self, base_url: str, api_key: Optional[str] = None, timeout: float = 10.0):
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self.timeout = timeout

    def _headers(self) -> Dict[str, str]:
        h = {"Content-Type": "application/json"}
        if self.api_key:
            h["Authorization"] = f"Bearer {self.api_key}"
        return h

    def _request(
        self,
        method: str,
        path: str,
        body: Optional[dict] = None,
        params: Optional[dict] = None,
    ) -> Any:
        url = f"{self.base_url}/api/v1{path}"
        if params:
            query = urllib.parse.urlencode({k: v for k, v in params.items() if v is not None})
            if query:
                url = f"{url}?{query}"
        data = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(url, data=data, method=method, headers=self._headers())
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                raw = resp.read()
                return json.loads(raw) if raw else {}
        except urllib.error.HTTPError as e:
            raise TaroHTTPError(e.code, e.read().decode(errors="replace")) from e
        except urllib.error.URLError as e:
            raise TaroHTTPError(0, str(e.reason)) from e

    def post(self, path: str, body: dict, params: Optional[dict] = None) -> Any:
        return self._request("POST", path, body=body, params=params)

    def get(self, path: str, params: Optional[dict] = None) -> Any:
        return self._request("GET", path, params=params)

    def patch(self, path: str, body: dict) -> Any:
        return self._request("PATCH", path, body=body)
