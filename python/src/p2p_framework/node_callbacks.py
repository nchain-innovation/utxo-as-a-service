from collections import defaultdict
import sys
import time
import logging
import traceback
from typing import Type, Dict, Any

from p2p_framework.util import wait_until
from p2p_framework.object import CProtoconf
from p2p_framework.message import msg_protoconf, msg_verack, msg_getdata, msg_ping, msg_pong

from p2p_framework.consensus import BIP0031_VERSION, MAX_PROTOCOL_RECV_PAYLOAD_LENGTH, MY_VERSION
from p2p_framework.network_thread import mininode_lock
from p2p_framework.node_connection import NodeConnection
logger = logging.getLogger("TestFramework.mininode")


class NodeCallbacks():
    """Callback and helper functions for P2P connection to a bitcoind node.

    Individual testcases should subclass this and override the on_* methods
    if they want to alter message handling behaviour.
    """

    def __init__(self):
        # Track whether we have a P2P connection open to the node
        self.connected: bool = False
        self.connection = None  # : Optional[Type[NodeConnection]]

        # Track number of messages of each type received and the most recent
        # message of each type
        self.message_count: Dict[str, int] = defaultdict(int)
        self.msg_timestamp: Dict[str, float] = {}
        self.last_message: Dict[str, Any] = {}
        self.time_index: int = 0
        self.msg_index: Dict[str, int] = defaultdict(int)

        # A count of the number of ping messages we've sent to the node
        self.ping_counter: int = 1

    # Message receiving methods

    def deliver(self, conn: Type[NodeConnection], message: Any):
        """Receive message and dispatch message to appropriate callback.

        We keep a count of how many of each message type has been received
        and the most recent message of each type.
        """

        with mininode_lock:
            try:
                command: str = message.command.decode('ascii')
                self.message_count[command] += 1
                self.last_message[command] = message
                self.msg_timestamp[command] = time.time()
                self.msg_index[command] = self.time_index
                self.time_index += 1
                getattr(self, 'on_' + command)(conn, message)
            except SystemExit:
                print("SystemExit")
                raise SystemExit
            except Exception as e:
                print(traceback.format_exc())
                print(sys.exc_info()[2])
                print(f"ERROR delivering {repr(message)}, {sys.exc_info()}, {e}")
                raise

    # Callback methods. Can be overridden by subclasses in individual test
    # cases to provide custom message handling behaviour.

    def on_open(self, conn) -> None:
        self.connected = True

    def on_close(self, conn) -> None:
        self.connected = False
        self.connection = None

    def on_addr(self, conn, message) -> None:
        pass

    def on_alert(self, conn, message) -> None:
        pass

    def on_block(self, conn, message) -> None:
        pass

    def on_blocktxn(self, conn, message) -> None:
        pass

    def on_cmpctblock(self, conn, message) -> None:
        pass

    def on_feefilter(self, conn, message) -> None:
        pass

    def on_getaddr(self, conn, message) -> None:
        pass

    def on_getblocks(self, conn, message) -> None:
        pass

    def on_getblocktxn(self, conn, message) -> None:
        pass

    def on_getdata(self, conn, message) -> None:
        pass

    def on_getheaders(self, conn, message) -> None:
        pass

    def on_headers(self, conn, message) -> None:
        pass

    def on_mempool(self, conn) -> None:
        pass

    def on_pong(self, conn, message) -> None:
        pass

    def on_reject(self, conn, message) -> None:
        pass

    def on_sendcmpct(self, conn, message) -> None:
        pass

    def on_sendheaders(self, conn, message) -> None:
        pass

    def on_tx(self, conn, message) -> None:
        pass

    def on_inv(self, conn, message) -> None:
        """ On receiving an inv message
            for each item in the inv_vect:
                if it is not an error:
                    append it to the list of msg_getdata
        """
        want = msg_getdata()
        for i in message.inv:
            if i.type != 0:
                want.inv.append(i)
        if len(want.inv):
            conn.send_message(want)

    def on_ping(self, conn, message) -> None:
        if conn.ver_send > BIP0031_VERSION:
            conn.send_message(msg_pong(message.nonce))

    def on_verack(self, conn, message) -> None:
        conn.ver_recv = conn.ver_send
        self.verack_received = True

    def on_streamack(self, conn, message) -> None:
        pass

    def on_protoconf(self, conn, message) -> None:
        pass

    def on_version(self, conn, message) -> None:
        if message.nVersion >= 209:
            conn.send_message(msg_verack())
            self.send_protoconf(conn)
        conn.ver_send = min(MY_VERSION, message.nVersion)
        if message.nVersion < 209:
            conn.ver_recv = conn.ver_send
        conn.nServices = message.nServices

    def on_notfound(self, conn, message) -> None:
        pass

    def send_protoconf(self, conn) -> None:
        conn.send_message(msg_protoconf(CProtoconf(2, MAX_PROTOCOL_RECV_PAYLOAD_LENGTH, b"BlockPriority,Default")))

    # Connection helper methods

    def add_connection(self, conn) -> None:
        self.connection = conn

    def wait_for_disconnect(self, timeout=60) -> None:
        def test_function() -> bool:
            return not self.connected
        wait_until(test_function, timeout=timeout, lock=mininode_lock)

    # Message receiving helper methods

    def clear_messages(self) -> None:
        with mininode_lock:
            self.message_count.clear()

    def wait_for_verack(self, timeout=60) -> None:
        def test_function() -> bool:
            return self.message_count["verack"] > 0
        wait_until(test_function, timeout=timeout, lock=mininode_lock)

    # Message sending helper functions

    def send_message(self, message: Any) -> None:
        if self.connection is not None:
            self.connection.send_message(message)
        else:
            logger.error("Cannot send message. No connection to node!")

    def send_and_ping(self, message) -> None:
        self.send_message(message)
        self.sync_with_ping()

    def sync_with_ping(self, timeout=60) -> None:
        """ Sync up with the node
            use ping to guarantee that previously sent p2p messages were processed
        """
        self.send_message(msg_ping(nonce=self.ping_counter))

        def test_function():
            if not self.last_message.get("pong"):
                return False
            if self.last_message["pong"].nonce != self.ping_counter:
                return False
            # after we receive pong we need to check that there are no async
            # block/transaction processes still running
            assert self.connection is not None
            if self.connection.rpc is not None:
                activity = self.connection.rpc.getblockchainactivity()
                return sum(activity.values()) == 0

        wait_until(test_function, timeout=timeout, lock=mininode_lock)
        self.ping_counter += 1
