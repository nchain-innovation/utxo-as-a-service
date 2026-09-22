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


class TestConfigSourceRequirements:
    """CS-407: both components must read the same database configuration.

    A source-contract test in the style of the P2P ones above. The two halves
    used to read different keys — Rust a URL from `[database]`, Python discrete
    host/port/user/password/database keys repeated under each network — and
    nothing kept them in agreement. `mysql_port` was set in no config file at
    all, so Python fell back to 3306 while Rust connected on 3307.

    They now read the same two keys, and this fails if either side stops.
    """

    KEYS = ("postgres_url", "postgres_url_docker")

    def test_cfg07_rust_reads_the_shared_database_keys(self) -> None:
        source = (REPO_ROOT / "rust/src/config.rs").read_text(encoding="utf-8")
        for key in self.KEYS:
            assert key in source, f"rust/src/config.rs no longer names {key}"

    def test_cfg07_python_reads_the_shared_database_keys(self) -> None:
        source = (REPO_ROOT / "python/src/database.py").read_text(encoding="utf-8")
        for key in self.KEYS:
            assert key in source, f"python/src/database.py no longer names {key}"

    def test_cfg07_both_configs_define_them_and_nothing_else(self) -> None:
        import toml

        for name in ("uaasr.toml", "uaasr.docker.toml"):
            config = toml.loads((REPO_ROOT / "data" / name).read_text(encoding="utf-8"))
            database = config["database"]
            for key in self.KEYS:
                assert key in database, f"{name} is missing [database].{key}"
            # The per-network keys are what allowed the two halves to diverge.
            network = config[config["service"]["network"]]
            for dead in ("host", "user", "password", "database", "mysql_port"):
                assert dead not in network, (
                    f"{name} still has a per-network '{dead}' key; that is the "
                    "second source of truth CS-407 removed"
                )

    def test_cfg07_both_choose_the_docker_url_the_same_way(self) -> None:
        # APP_ENV selects the in-container URL on both sides. If one changed to
        # a different signal they would silently target different databases.
        rust = (REPO_ROOT / "rust/src/config.rs").read_text(encoding="utf-8")
        python = (REPO_ROOT / "python/src/database.py").read_text(encoding="utf-8")
        # Quoted, so this is the whole variable name. A bare substring check
        # passes for APP_ENVIRONMENT, which is a different variable.
        assert '"APP_ENV"' in rust
        assert '"APP_ENV"' in python

    def test_cfg08_both_honour_the_same_url_override(self) -> None:
        # UAAS_POSTGRES_URL is what lets the tracked configs carry a
        # placeholder. If only one side read it, that side would reach the
        # database and the other would refuse to start — or worse, the two
        # would target different databases (CS-450).
        rust = (REPO_ROOT / "rust/src/config.rs").read_text(encoding="utf-8")
        python = (REPO_ROOT / "python/src/database.py").read_text(encoding="utf-8")
        assert '"UAAS_POSTGRES_URL"' in rust
        assert '"UAAS_POSTGRES_URL"' in python

    def test_cfg09_no_tracked_config_carries_a_working_credential(self) -> None:
        # The repository is public. A password or a real peer address here is
        # disclosed the moment it is pushed, and removing it later does not
        # remove it from history (CS-450).
        import toml

        for name in ("uaasr.toml", "uaasr.docker.toml"):
            text = (REPO_ROOT / "data" / name).read_text(encoding="utf-8")
            config = toml.loads(text)
            for key in ("postgres_url", "postgres_url_docker"):
                assert "CHANGE-ME" in config["database"][key], (
                    f"{name} [database].{key} looks like a real URL; it must "
                    "carry the CHANGE-ME placeholder and be supplied through "
                    "UAAS_POSTGRES_URL"
                )
            # Peer addresses are deliberately NOT asserted on. A node address
            # is not a credential — nodes gossip each other's addresses and
            # public crawlers list reachable ones — and a placeholder there
            # means the stack starts and silently never syncs, which is worse
            # for development. Revisit before a live deployment; 192.0.2.1 is
            # the unroutable value the service warns about.
