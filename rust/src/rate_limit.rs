use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

const WINDOW: Duration = Duration::from_secs(60);

pub struct RateLimiter {
    limit_per_minute: u32,
    windows: Mutex<HashMap<String, (Instant, u32)>>,
}

impl RateLimiter {
    pub fn new(limit_per_minute: u32) -> Self {
        RateLimiter {
            limit_per_minute,
            windows: Mutex::new(HashMap::new()),
        }
    }

    pub fn allow(&self, client: &str) -> bool {
        if self.limit_per_minute == 0 {
            return true;
        }

        let mut guard = match self.windows.lock() {
            Ok(guard) => guard,
            Err(err) => {
                log::error!("Rate limiter lock poisoned: {err}");
                return false;
            }
        };

        let now = Instant::now();
        let entry = guard.entry(client.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) >= WINDOW {
            *entry = (now, 0);
        }
        if entry.1 >= self.limit_per_minute {
            return false;
        }
        entry.1 += 1;
        true
    }
}

pub fn client_ip(req: &actix_web::HttpRequest) -> String {
    if let Some(forwarded) = req
        .headers()
        .get("X-Forwarded-For")
        .and_then(|value| value.to_str().ok())
    {
        return forwarded
            .split(',')
            .next()
            .unwrap_or(forwarded)
            .trim()
            .to_string();
    }
    req.connection_info()
        .peer_addr()
        .unwrap_or("unknown")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::RateLimiter;

    #[test]
    fn disabled_limiter_allows_all() {
        let limiter = RateLimiter::new(0);
        assert!(limiter.allow("127.0.0.1"));
        assert!(limiter.allow("127.0.0.1"));
    }

    #[test]
    fn enforces_limit_per_window() {
        let limiter = RateLimiter::new(2);
        assert!(limiter.allow("client-a"));
        assert!(limiter.allow("client-a"));
        assert!(!limiter.allow("client-a"));
        assert!(limiter.allow("client-b"));
    }
}
