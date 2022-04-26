from __future__ import annotations
import time


class Timer:
    """ Class to capture elapsed time
    """
    def __init__(self):
        self.start_time: float = time.monotonic()

    def start(self) -> Timer:
        self.start_time = time.monotonic()
        return self

    def elapsed(self) -> float:
        return time.monotonic() - self.start_time
