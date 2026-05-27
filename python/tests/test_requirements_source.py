from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]


class TestP2PSourceRequirements:
    def test_sync01_peer_connection_uses_configured_network_port(self) -> None:
        source = (REPO_ROOT / "rust/src/peer_connection.rs").read_text(encoding="utf-8")
        assert "get_network_settings" in source
        assert "settings.port" in source

    def test_sync02_thread_manager_cycles_configured_ips(self) -> None:
        source = (REPO_ROOT / "rust/src/main.rs").read_text(encoding="utf-8")
        assert "into_iter().cycle()" in source

    def test_sync03_out_of_order_blocks_are_queued(self) -> None:
        source = (REPO_ROOT / "rust/src/uaas/block_manager.rs").read_text(encoding="utf-8")
        assert "block_queue" in source
        assert "prev_hash == self.last_hash_processed" in source

    def test_sync09_connect_events_are_logged(self) -> None:
        source = (REPO_ROOT / "rust/src/uaas/connection.rs").read_text(encoding="utf-8")
        assert "INSERT INTO connect" in source
        assert "on_connect" in source
        assert "on_disconnect" in source

    def test_rel04_failed_peer_connection_is_logged_not_fatal(self) -> None:
        source = (REPO_ROOT / "rust/src/thread_manager.rs").read_text(encoding="utf-8")
        assert "Unable to create peer connection" in source
        assert "return;" in source

    def test_rel01_shutdown_sends_stop_to_peer_manager(self) -> None:
        source = (REPO_ROOT / "rust/src/main.rs").read_text(encoding="utf-8")
        assert "wait_for_shutdown_signal" in source
        assert "PeerEventType::Stop" in source
