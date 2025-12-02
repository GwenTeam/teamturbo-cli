use anyhow::Result;
use console::style;
use dialoguer::Input;
use reqwest::Client;
use crate::auth::{AuthConfig, VerifyResponse};

/// Authorize via manual token input (mode 2)
pub async fn authorize(base_url: &str) -> Result<AuthConfig> {
    let offline_url = format!("{}/cli/offline_login", base_url);

    println!("{}", style("Unable to open browser automatically.").yellow());
    println!("\n{}", style("Please follow these steps:").cyan().bold());
    println!("  1. Visit this URL in a browser:");
    println!("     {}", style(&offline_url).yellow().underlined());
    println!("  2. Click 'Generate CLI Token'");
    println!("  3. Copy the token and paste it below\n");

    // Prompt for token
    let token: String = Input::new()
        .with_prompt("Paste the token here")
        .interact_text()?;

    let token = token.trim().to_string();

    if token.is_empty() {
        anyhow::bail!("Token cannot be empty");
    }

    println!("{}", style("Verifying token...").cyan());

    // Verify token
    let client = Client::new();
    let verify_url = format!("{}/api/cli/auth/verify", base_url);

    let response = client
        .get(&verify_url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!("Token verification failed: Invalid or expired token");
    }

    let verify_data: VerifyResponse = response.json().await?;

    if !verify_data.valid {
        anyhow::bail!("Token is invalid or expired");
    }

    if let (Some(user), Some(expires_at)) = (verify_data.user, verify_data.expires_at) {
        println!("{}", style("✓ Token verified successfully!").green().bold());
        println!("  {} {} ({})",
            style("Logged in as:").dim(),
            style(&user.display_name).cyan().bold(),
            style(&user.email).dim()
        );

        Ok(AuthConfig {
            access_token: token,
            token_type: "Bearer".to_string(),
            expires_at,
            user_id: user.id,
            user_name: user.display_name,
            user_email: user.email,
        })
    } else {
        anyhow::bail!("Failed to get user information from verification response");
    }
}
