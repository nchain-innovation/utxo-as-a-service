import time
from threading import Lock
from typing import Dict, Tuple

from starlette.requests import Request

WINDOW_SECONDS = 60.0


def client_ip(request: Request) -> str:
    forwarded = request.headers.get("x-forwarded-for")
    if forwarded:
        return forwarded.split(",")[0].strip()
    if request.client:
        return request.client.host
    return "unknown"


class FixedWindowRateLimiter:
    """Per-client fixed-window request counter (thread-safe)."""

    def __init__(self, limit_per_minute: int) -> None:
        self.limit_per_minute = limit_per_minute
        self._windows: Dict[str, Tuple[float, int]] = {}
        self._lock = Lock()

    def allow(self, client: str) -> bool:
        if self.limit_per_minute == 0:
            return True

        now = time.monotonic()
        with self._lock:
            window_start, count = self._windows.get(client, (now, 0))
            if now - window_start >= WINDOW_SECONDS:
                window_start = now
                count = 0
            if count >= self.limit_per_minute:
                self._windows[client] = (window_start, count)
                return False
            self._windows[client] = (window_start, count + 1)
            return True
