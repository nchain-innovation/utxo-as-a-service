#!/usr/bin/env python3
# Copyright (c) 2010 ArtForz -- public domain half-a-node
# Copyright (c) 2012 Jeff Garzik
# Copyright (c) 2010-2016 The Bitcoin Core developers
# Copyright (c) 2019 Bitcoin Association
# Distributed under the Open BSV software license, see the accompanying file LICENSE.

"""Bitcoin P2P network half-a-node.

This python code was modified from ArtForz' public domain  half-a-node, as
found in the mini-node branch of http://github.com/jgarzik/pynode.

NodeConn: an object which manages p2p connectivity to a bitcoin node
NodeConnCB: a base class that describes the interface for receiving
            callbacks with network messages from a NodeConn
CBlock, CTransaction, CBlockHeader, CTxIn, CTxOut, etc....:
    data structures that should map to corresponding structures in
    bitcoin/primitives
msg_block, msg_tx, msg_headers, etc.:
    data structures that represent network messages
ser_*, deser_*: functions that handle serialization/deserialization
"""

import asyncore
import logging
from threading import RLock, Thread
from typing import Dict, Any


logger = logging.getLogger("TestFramework.mininode")

# Keep our own socket map for asyncore, so that we can track disconnects
# ourselves (to workaround an issue with closing an asyncore socket when
# using select)
mininode_socket_map: Dict[int, Any] = dict()

# One lock for synchronizing all data access between the networking thread (see
# NetworkThread below) and the thread running the test logic.  For simplicity,
# NodeConn acquires this lock whenever delivering a message to a NodeConnCB,
# and whenever adding anything to the send buffer (in send_message()).  This
# lock should be acquired in the thread running the test logic to synchronize
# access to any data shared with the NodeConnCB or NodeConn.
mininode_lock: RLock = RLock()

# Lock used to synchronize access to data required by loop running in NetworkThread.
# It must be locked, for example, when adding new NodeConn object, otherwise loop in
# NetworkThread may try to access partially constructed object.
network_thread_loop_lock: RLock = RLock()

# Network thread acquires network_thread_loop_lock at start of each iteration and releases
# it at the end. Since the next iteration is run immediately after that, lock is acquired
# almost all of the time making it difficult for other threads to also acquire this lock.
# To work around this problem, NetworkThread first acquires network_thread_loop_intent_lock
# and immediately releases it before acquiring network_thread_loop_lock.
# Other threads (e.g. the ones calling NodeConn constructor) acquire both locks before
# proceeding. The end result is that other threads wait at most one iteration of loop in
# NetworkThread.
network_thread_loop_intent_lock: RLock = RLock()


NetworkThread_should_stop: bool
NetworkThread_should_stop = False


def StopNetworkThread():
    global NetworkThread_should_stop
    NetworkThread_should_stop = True


class NetworkThread(Thread):

    poll_timeout = 0.1

    def run(self):
        while mininode_socket_map and not NetworkThread_should_stop:
            with network_thread_loop_intent_lock:
                """ Acquire and immediately release lock.
                    This allows other threads to more easily acquire network_thread_loop_lock by
                    acquiring (and holding) network_thread_loop_intent_lock first since NetworkThread
                    will block on trying to acquire network_thread_loop_intent_lock in the line above.
                    If this was not done, other threads would need to wait for a long time (>10s) for
                    network_thread_loop_lock since it is released only briefly between two loop iterations.
                """
                pass
            with network_thread_loop_lock:
                """ We check for whether to disconnect outside of the asyncore
                    loop to workaround the behavior of asyncore when using
                    select
                """
                disconnected = []
                for fd, obj in mininode_socket_map.items():
                    if obj.disconnect:
                        disconnected.append(obj)
                [obj.handle_close() for obj in disconnected]
                try:
                    asyncore.loop(NetworkThread.poll_timeout, use_poll=True, map=mininode_socket_map, count=1)
                except Exception as e:
                    """ All exceptions are caught to prevent them from taking down the network thread.
                        Since the error cannot be easily reported, it is just logged assuming that if
                        the error is relevant, the test will detect it in some other way.
                    """
                    logger.warning("mininode NetworkThread: asyncore.loop() failed! " + str(e))
        logger.debug("Network thread closing")
