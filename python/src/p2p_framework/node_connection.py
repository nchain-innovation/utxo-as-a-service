
import asyncore
import socket
import struct
import time
import logging
from io import BytesIO
from typing import Any

from p2p_framework.hash import sha256
from p2p_framework.object import CInv
from p2p_framework.message import (
    msg_version, msg_protoconf, msg_verack, msg_createstream, msg_streamack, msg_addr,
    msg_alert, msg_inv, msg_getdata, msg_getblocks, msg_tx, msg_block, msg_getaddr,
    msg_ping, msg_pong, msg_headers, msg_getheaders, msg_reject, msg_mempool, msg_feefilter, msg_sendheaders,
    msg_sendcmpct, msg_cmpctblock, msg_getblocktxn, msg_blocktxn, msg_notfound, msg_ping_prebip31)

from p2p_framework.consensus import READ_BUFFER_SIZE, BIP0031_VERSION, LEGACY_MAX_PROTOCOL_PAYLOAD_LENGTH, NODE_NETWORK

from p2p_framework.network_thread import network_thread_loop_intent_lock, network_thread_loop_lock, mininode_lock, mininode_socket_map


LOGGER = logging.getLogger(__name__)


class NodeConnection(asyncore.dispatcher):
    """ The actual NodeConnection class
        This class provides an interface for a p2p connection to a specified node
    """
    messagemap = {
        b"version": msg_version,
        b"protoconf": msg_protoconf,
        b"verack": msg_verack,
        b"createstrm": msg_createstream,
        b"streamack": msg_streamack,
        b"addr": msg_addr,
        b"alert": msg_alert,
        b"inv": msg_inv,
        b"getdata": msg_getdata,
        b"getblocks": msg_getblocks,
        b"tx": msg_tx,
        b"block": msg_block,
        b"getaddr": msg_getaddr,
        b"ping": msg_ping,
        b"pong": msg_pong,
        b"headers": msg_headers,
        b"getheaders": msg_getheaders,
        b"reject": msg_reject,
        b"mempool": msg_mempool,
        b"feefilter": msg_feefilter,
        b"sendheaders": msg_sendheaders,
        b"sendcmpct": msg_sendcmpct,
        b"cmpctblock": msg_cmpctblock,
        b"getblocktxn": msg_getblocktxn,
        b"blocktxn": msg_blocktxn,
        b"notfound": msg_notfound
    }

    MAGIC_BYTES = {
        "mainnet": b"\xe3\xe1\xf3\xe8",
        "testnet3": b"\xf4\xe5\xf3\xf4",
        "stn": b"\xfb\xce\xc4\xf9",
        "regtest": b"\xda\xb5\xbf\xfa",
    }

    def __init__(self, dstaddr: str, dstport: int, callback, net="regtest", services=NODE_NETWORK, send_version=True,
                 strSubVer=None, assocID=None, nullAssocID=False):
        """ Lock must be acquired when new object is added to prevent NetworkThread from trying
            to access partially constructed object or trying to call callbacks before the connection
            is established.
        """
        with network_thread_loop_intent_lock, network_thread_loop_lock:
            asyncore.dispatcher.__init__(self, map=mininode_socket_map)
            self.dstaddr = dstaddr
            self.dstport = dstport
            self.create_socket(socket.AF_INET, socket.SOCK_STREAM)
            self.sendbuf: bytearray = bytearray()
            self.recvbuf: bytes = b""
            self.ver_send: int = 209
            self.ver_recv = 209
            self.last_sent: float = 0.0
            self.state: str = "connecting"
            self.network: str = net
            self.cb = callback
            self.disconnect: bool = False
            self.nServices: int = 0
            self.maxInvElements: int = CInv.estimateMaxInvElements(LEGACY_MAX_PROTOCOL_PAYLOAD_LENGTH)
            self.strSubVer: str = strSubVer
            self.assocID = assocID

            if assocID is not None:
                send_version = False

            if send_version:
                # stuff version msg into sendbuf
                vt = msg_version()
                vt.nServices = services
                vt.addrTo.ip = self.dstaddr
                vt.addrTo.port = self.dstport
                assert vt.addrFrom is not None
                vt.addrFrom.ip = "0.0.0.0"
                vt.addrFrom.port = 0
                if(strSubVer):
                    vt.strSubVer = strSubVer
                if(nullAssocID):
                    vt.assocID = None
                self.send_message(vt, True)
                self.assocID = vt.assocID
            try:
                self.connect((dstaddr, dstport))
            except:
                self.handle_close()

    def handle_connect(self):
        if self.state != "connected":
            self.state = "connected"
            self.cb.on_open(self)

    def handle_close(self):
        self.state = "closed"
        self.recvbuf = b""
        self.sendbuf = bytearray()
        try:
            self.close()
        except:
            pass
        self.cb.on_close(self)

    def handle_read(self) -> None:
        with mininode_lock:
            t = self.recv(READ_BUFFER_SIZE)
            if len(t) > 0:
                self.recvbuf += t

        while True:
            msg = self.got_data()
            if msg is None:
                break
            self.got_message(msg)

    def readable(self) -> bool:
        return True

    def writable(self) -> bool:
        with mininode_lock:
            pre_connection = self.state == "connecting"
            length = len(self.sendbuf)
        return (length > 0 or pre_connection)

    def handle_write(self) -> None:
        with mininode_lock:
            # asyncore does not expose socket connection, only the first read/write
            # event, thus we must check connection manually here to know when we
            # actually connect
            if self.state == "connecting":
                self.handle_connect()
            if not self.writable():
                return

            try:
                sent = self.send(self.sendbuf)
            except:
                self.handle_close()
                return
            del self.sendbuf[:sent]

    def got_data(self) -> Any:
        try:
            with mininode_lock:
                if len(self.recvbuf) < 4:
                    return None
                if self.recvbuf[:4] != self.MAGIC_BYTES[self.network]:
                    raise ValueError("got garbage %s" % repr(self.recvbuf))
                if self.ver_recv < 209:
                    if len(self.recvbuf) < 4 + 12 + 4:
                        return None
                    command = self.recvbuf[4:4 + 12].split(b"\x00", 1)[0]
                    payloadlen = struct.unpack(
                        "<i", self.recvbuf[4 + 12:4 + 12 + 4])[0]
                    checksum = None
                    if len(self.recvbuf) < 4 + 12 + 4 + payloadlen:
                        return None
                    msg = self.recvbuf[4 + 12 + 4:4 + 12 + 4 + payloadlen]
                    self.recvbuf = self.recvbuf[4 + 12 + 4 + payloadlen:]
                else:
                    if len(self.recvbuf) < 4 + 12 + 4 + 4:
                        return None
                    command = self.recvbuf[4:4 + 12].split(b"\x00", 1)[0]
                    payloadlen = struct.unpack(
                        "<i", self.recvbuf[4 + 12:4 + 12 + 4])[0]
                    checksum = self.recvbuf[4 + 12 + 4:4 + 12 + 4 + 4]
                    if len(self.recvbuf) < 4 + 12 + 4 + 4 + payloadlen:
                        return None
                    msg = self.recvbuf[4 + 12 + 4 + 4:4 + 12 + 4 + 4 + payloadlen]
                    h = sha256(sha256(msg))
                    if checksum != h[:4]:
                        raise ValueError(
                            "got bad checksum " + repr(self.recvbuf))
                    self.recvbuf = self.recvbuf[4 + 12 + 4 + 4 + payloadlen:]
                if command not in self.messagemap:
                    LOGGER.warning(f"Received unknown command from {self.dstaddr}:{self.dstport}: '{repr(command)}' {repr(msg)}")
                    # logger.warning("Received unknown command from %s:%d: '%s' %s" % (
                    #    self.dstaddr, self.dstport, command, repr(msg)))
                    raise ValueError("Unknown command: '%r'" % (command))
                f = BytesIO(msg)
                m = self.messagemap[command]()
                m.deserialize(f)
                return m

        except Exception as e:
            LOGGER.exception('got_data:', repr(e))
            raise

    def send_message(self, message, pushbuf=False) -> None:
        if self.state != "connected" and not pushbuf:
            raise IOError('Not connected, no pushbuf')
        self._log_message("send", message)
        command = message.command
        data = message.serialize()
        tmsg = self.MAGIC_BYTES[self.network]
        tmsg += command
        tmsg += b"\x00" * (12 - len(command))
        tmsg += struct.pack("<I", len(data))
        if self.ver_send >= 209:
            th = sha256(data)
            h = sha256(th)
            tmsg += h[:4]
        tmsg += data
        with mininode_lock:
            self.sendbuf += tmsg
            self.last_sent = time.monotonic()

    def got_message(self, message: Any) -> None:
        if message.command == b"version":
            if message.nVersion <= BIP0031_VERSION:
                self.messagemap[b'ping'] = msg_ping_prebip31
        # Every 30 mins
        if self.last_sent + 30 * 60 < time.monotonic():
            self.send_message(self.messagemap[b'ping']())
        self._log_message("receive", message)
        self.cb.deliver(self, message)

    def _log_message(self, direction: str, msg: Any) -> None:
        if direction == "send":
            log_message = "Send message to "
        elif direction == "receive":
            log_message = "Received message from "
        log_message += "%s:%d: %s" % (self.dstaddr,
                                      self.dstport, repr(msg)[:500])
        if len(log_message) > 500:
            log_message += "... (msg truncated)"
        LOGGER.debug(log_message)

    def disconnect_node(self) -> None:
        self.disconnect = True
