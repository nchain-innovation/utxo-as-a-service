from rate_limit import FixedWindowRateLimiter, client_ip


class TestFixedWindowRateLimiter:
    def test_disabled_allows_all(self) -> None:
        limiter = FixedWindowRateLimiter(0)
        assert limiter.allow("127.0.0.1")
        assert limiter.allow("127.0.0.1")

    def test_enforces_limit_per_client(self) -> None:
        limiter = FixedWindowRateLimiter(2)
        assert limiter.allow("client-a")
        assert limiter.allow("client-a")
        assert not limiter.allow("client-a")
        assert limiter.allow("client-b")


class TestClientIp:
    def test_uses_forwarded_for(self) -> None:
        class Client:
            host = "10.0.0.1"

        class Request:
            headers = {"x-forwarded-for": "203.0.113.5, 10.0.0.1"}
            client = Client()

        assert client_ip(Request()) == "203.0.113.5"

    def test_falls_back_to_peer(self) -> None:
        class Client:
            host = "127.0.0.1"

        class Request:
            headers = {}
            client = Client()

        assert client_ip(Request()) == "127.0.0.1"
