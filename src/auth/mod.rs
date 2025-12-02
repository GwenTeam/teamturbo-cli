pub mod browser;
pub mod manual;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthConfig {
    pub access_token: String,
    pub token_type: String,
    pub expires_at: String,
    pub user_id: i64,
    pub user_name: String,
    pub user_email: String,
}

#[derive(Debug, Deserialize)]
pub struct PollResponse {
    pub status: i32,
    pub error: Option<String>,
    pub error_msg: Option<String>,
    pub data: Option<PollData>,
    pub auth: Option<AuthData>,
}

#[derive(Debug, Deserialize)]
pub struct PollData {
    pub status: String,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AuthData {
    pub status: String,
    pub access_token: Option<String>,
    pub token_type: Option<String>,
    pub expires_at: Option<String>,
    pub user: Option<User>,
}

#[derive(Debug, Deserialize)]
pub struct User {
    pub id: i64,
    pub account: String,
    pub display_name: String,
    pub email: String,
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub expires_at: String,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct VerifyResponse {
    pub valid: bool,
    pub user: Option<User>,
    pub expires_at: Option<String>,
}

/// Generate a random login ID
pub fn generate_login_id() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();

    (0..32)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Check if browser can be opened
pub fn can_open_browser() -> bool {
    std::env::var("DISPLAY").is_ok() || cfg!(target_os = "windows") || cfg!(target_os = "macos")
}
