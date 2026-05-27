use std::panic::{catch_unwind, AssertUnwindSafe};

/// Run `f`, logging and swallowing panics so caller threads keep running.
pub fn catch_unwind_logged<F, R>(label: &str, f: F) -> Option<R>
where
    F: FnOnce() -> R,
{
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => Some(value),
        Err(payload) => {
            if let Some(message) = payload.downcast_ref::<&str>() {
                log::error!("Thread panic in {label}: {message}");
            } else if let Some(message) = payload.downcast_ref::<String>() {
                log::error!("Thread panic in {label}: {message}");
            } else {
                log::error!("Thread panic in {label}: unknown payload");
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::catch_unwind_logged;

    #[test]
    fn returns_some_on_success() {
        assert_eq!(catch_unwind_logged("test", || 42), Some(42));
    }

    #[test]
    fn returns_none_on_panic() {
        assert!(catch_unwind_logged("test", || panic!("boom")).is_none());
    }
}
