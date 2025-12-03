use anyhow::{Result, bail};
use console::style;
use std::time::Duration;
use reqwest::Client;
use crate::auth::{generate_login_id, AuthConfig, PollResponse};
use crate::utils::logger;

/// Authorize via browser (mode 1)
pub async fn authorize(base_url: &str) -> Result<AuthConfig> {
    let login_id = generate_login_id();

    // Determine frontend and backend URLs
    // Development mode: different ports for frontend and backend
    let (frontend_url, backend_url) = if base_url.contains("://127.0.0.1") || base_url.contains("://localhost") {
        // Extract port from base_url
        if let Some(port_start) = base_url.rfind(':') {
            let port_str = &base_url[port_start + 1..];
            let frontend_port: u16 = port_str.parse().unwrap_or(3100);

            // Map known development frontend ports to backend ports
            // Only map Vite dev server (3100) to Rails backend (3001)
            // For other ports, assume frontend and backend are on the same server
            let backend_port = match frontend_port {
                3100 => 3001,  // Standard Vite frontend -> Rails backend
                _ => frontend_port,  // Same server for all other cases
            };

            let backend = if backend_port != frontend_port {
                base_url.replace(&format!(":{}", frontend_port), &format!(":{}", backend_port))
            } else {
                base_url.to_string()
            };

            (base_url.to_string(), backend)
        } else {
            // No port specified, assume production
            (base_url.to_string(), base_url.to_string())
        }
    } else {
        // Production mode: same URL for both
        (base_url.to_string(), base_url.to_string())
    };

    // Initialize login session on server
    let client = Client::new();
    let init_url = format!("{}/api/cli/auth/init", backend_url);

    println!("{}", style("Initializing login session...").cyan());

    let init_response = client
        .post(&init_url)
        .json(&serde_json::json!({ "login_id": login_id }))
        .send()
        .await?;

    if !init_response.status().is_success() {
        bail!("Failed to initialize login session: {}", init_response.status());
    }

    let auth_url = format!("{}/cli-auth?login_id={}", frontend_url, login_id);

    println!("{}", style("Opening browser for authorization...").cyan());

    // Open browser
    if let Err(e) = webbrowser::open(&auth_url) {
        eprintln!("{}", style(format!("Failed to open browser: {}", e)).red());
        println!("\nPlease manually open this URL in your browser:");
        println!("{}", style(&auth_url).yellow());
    }

    println!("{}", style("Waiting for authorization... (Press Ctrl+C to cancel)").cyan());

    // Poll for authorization
    let poll_url = format!("{}/api/cli/auth/poll", backend_url);

    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;

        if logger::is_verbose() {
            println!("\n[DEBUG] Polling URL: {}", poll_url);
            println!("[DEBUG] Login ID: {}", login_id);
        }

        let response = client
            .get(&poll_url)
            .query(&[("login_id", &login_id)])
            .send()
            .await?;

        if logger::is_verbose() {
            println!("[DEBUG] Response status: {}", response.status());
        }

        // Get response body as text first for debugging
        let body_text = response.text().await?;

        if logger::is_verbose() {
            println!("[DEBUG] Response body: {}", body_text);
        }

        // Try to parse the response
        if body_text.is_empty() {
            if logger::is_verbose() {
                println!("[DEBUG] Empty response body, continuing...");
            }
            continue;
        }

        let data: PollResponse = match serde_json::from_str(&body_text) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[ERROR] Failed to parse JSON: {}", e);
                eprintln!("[ERROR] Raw body: {}", body_text);
                bail!("Failed to parse server response");
            }
        };

        // Handle the response based on status code and data
        if data.status == 0 {
            if let Some(auth) = data.auth {
                match auth.status.as_str() {
                    "authorized" => {
                        if let (Some(token), Some(user), Some(expires_at)) =
                            (auth.access_token, auth.user, auth.expires_at) {

                            println!("{}", style("✓ Authorization successful!").green().bold());
                            println!("  {} {} ({})",
                                style("Logged in as:").dim(),
                                style(&user.display_name).cyan().bold(),
                                style(&user.email).dim()
                            );

                            return Ok(AuthConfig {
                                access_token: token,
                                token_type: auth.token_type.unwrap_or_else(|| "Bearer".to_string()),
                                expires_at,
                                user_id: user.id,
                                user_name: user.display_name,
                                user_email: user.email,
                            });
                        }
                    }
                    "denied" => {
                        bail!("Authorization was denied by user");
                    }
                    "pending" => {
                        // Continue waiting
                        if logger::is_verbose() {
                            println!("[DEBUG] Status: pending, continuing to poll...");
                        }
                        continue;
                    }
                    _ => {
                        bail!("Unknown authorization status: {}", auth.status);
                    }
                }
            } else {
                // status == 0 but no auth data (e.g., pending with status 202)
                if logger::is_verbose() {
                    println!("[DEBUG] No auth data yet, continuing to poll...");
                }
                continue;
            }
        } else {
            // status != 0, error case
            eprintln!("[ERROR] Server returned error status: {}", data.status);
            if let Some(err) = data.error {
                bail!("Authorization failed: {}", err);
            } else {
                bail!("Authorization failed with status code: {}", data.status);
            }
        }
    }
}
