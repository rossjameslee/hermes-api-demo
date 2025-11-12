use tracing::trace;

// Lightweight metrics helpers that are safe in demo builds.
// These intentionally avoid pulling in metrics macros to keep deps stable.

pub fn inc_requests(route: &'static str) {
    trace!(
        target = "hermes.metrics",
        route = route,
        "requests_total_inc"
    );
}

pub fn stage_elapsed(stage: &'static str, elapsed_ms: u128) {
    trace!(
        target = "hermes.metrics",
        stage = stage,
        elapsed_ms = elapsed_ms as u64,
        "stage_elapsed"
    );
}
