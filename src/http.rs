use reqwest::Client;
use std::time::Duration;

pub fn build_client() -> Client {
    let timeout = std::env::var("HTTP_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(15);
    let connect = std::env::var("HTTP_CONNECT_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(5);
    Client::builder()
        .timeout(Duration::from_secs(timeout))
        .connect_timeout(Duration::from_secs(connect))
        .build()
        .unwrap_or_else(|_| Client::new())
}
