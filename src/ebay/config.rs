#![allow(dead_code)]

use once_cell::sync::Lazy;
use std::env;

pub static EBAY_ENV: Lazy<String> =
    Lazy::new(|| env::var("EBAY_ENV").unwrap_or_else(|_| "SANDBOX".to_string()));

pub static APP_ID: Lazy<String> =
    Lazy::new(|| env::var("EBAY_APP_ID_PRODUCTION").unwrap_or_default());

pub static APP_SECRET: Lazy<String> =
    Lazy::new(|| env::var("EBAY_CERT_ID_PRODUCTION").unwrap_or_default());

pub static EBAY_REFRESH_TOKEN: Lazy<String> =
    Lazy::new(|| env::var("EBAY_REFRESH_TOKEN").unwrap_or_default());

pub static DEFAULT_CATEGORY_TREE_ID: Lazy<String> =
    Lazy::new(|| env::var("EBAY_CATEGORY_TREE_ID").unwrap_or_else(|_| "0".to_string()));

pub static ROOT: Lazy<String> = Lazy::new(|| {
    if EBAY_ENV.as_str().eq_ignore_ascii_case("PROD") {
        "https://api.ebay.com".to_string()
    } else {
        "https://api.sandbox.ebay.com".to_string()
    }
});

pub static OAUTH_TOKEN_URL: Lazy<String> =
    Lazy::new(|| format!("{}/identity/v1/oauth2/token", *ROOT));
