//! Single seam for gating a feature behind a future license/subscription
//! check. Everything is on for everyone today — this exists so that when
//! billing arrives, it plugs in here instead of into every call site that
//! cares.

pub fn enabled(_id: &str) -> bool {
    true
}
