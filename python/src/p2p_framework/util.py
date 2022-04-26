#!/usr/bin/env python3
# Copyright (c) 2014-2016 The Bitcoin Core developers
# Copyright (c) 2019 Bitcoin Association
# Distributed under the Open BSV software license, see the accompanying file LICENSE.
"""Helpful routines for regression testing."""

from binascii import hexlify, unhexlify
import time
from typing import Callable


def bytes_to_hex_str(byte_str: bytes) -> str:
    return hexlify(byte_str).decode('ascii')


def hex_str_to_bytes(hex_str: str) -> bytes:
    return unhexlify(hex_str.encode('ascii'))


def int_to_hex_str(val: int) -> str:
    """ As used by TxIDs
    """
    return f"{val:064x}"


def hex_str_to_int(hs: str) -> int:
    return int(hs, 16)


def wait_until(predicate: Callable[..., bool], *, attempts=float('inf'), timeout=float('inf'), lock=None, check_interval=0.05, label="wait_until"):
    """ wait until one of
        * the predicate returns true
        * number of attempts is exceededr
        * timeout occurs
        if lock is provided set that prior to calling predicate
    """
    if attempts == float('inf') and timeout == float('inf'):
        timeout = 60
    attempt: int = 0
    timestamp: float = timeout + time.monotonic()

    while attempt < attempts and time.monotonic() < timestamp:
        if lock:
            with lock:
                if predicate():
                    return
        else:
            if predicate():
                return
        attempt += 1
        time.sleep(check_interval)

    # Print the cause of the timeout
    assert attempts > attempt, f"{label} : max attempts exceeeded (attempts={attempt})"
    assert timestamp >= time.time(), f"{label} : timeout exceeded {timeout}"
    raise RuntimeError('Unreachable')
