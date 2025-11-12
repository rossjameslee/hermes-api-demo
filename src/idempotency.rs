use crate::models::ListingResponse;
use redis::AsyncCommands;

pub async fn redis_get(client: &redis::Client, key: &str) -> Option<ListingResponse> {
    let mut conn = match client.get_multiplexed_async_connection().await {
        Ok(c) => c,
        Err(_) => return None,
    };
    let s: Option<String> = conn.get(key).await.ok();
    s.and_then(|v| serde_json::from_str(&v).ok())
}

pub async fn redis_set(
    client: &redis::Client,
    key: &str,
    value: &ListingResponse,
    ttl_secs: usize,
) {
    if let Ok(mut conn) = client.get_multiplexed_async_connection().await
        && let Ok(json) = serde_json::to_string(value)
    {
        let _: Result<(), _> = conn.set_ex(key, json, ttl_secs as u64).await;
    }
}
