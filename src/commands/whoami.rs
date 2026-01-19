use anyhow::Result;
use console::style;
use chrono::{DateTime, Utc};
use crate::config::CliConfig;
use crate::api::ApiClient;

pub async fn execute() -> Result<()> {
    println!("{}", style("TeamTurbo CLI Status").cyan().bold());
    println!();

    // Load config
    let config = CliConfig::load()?;

    // Check if there are any saved auth configs
    if config.auth.is_empty() {
        println!("{}", style("Not logged in").yellow());
        println!();
        println!("{}", style("Run 'teamturbo login' to authenticate").dim());
        return Ok(());
    }

    // Verify each server
    for (server_url, auth_config) in config.auth.iter() {
        println!("{}", style(format!("Server: {}", server_url)).bold());

        let client = ApiClient::new(server_url.clone(), auth_config.access_token.clone());

        match client.verify().await {
            Ok(verify_response) => {
                println!("  {}: {}", style("Status").dim(), style("✓ Active").green());
                println!("  {}: {} ({})",
                    style("User").dim(),
                    verify_response.user.display_name_or_account(),
                    verify_response.user.account
                );
                println!("  {}: {}",
                    style("User ID").dim(),
                    verify_response.user.id
                );

                // Parse and format expiry date
                if let Ok(expires_at) = DateTime::parse_from_rfc3339(&verify_response.expires_at) {
                    let now = Utc::now();
                    let expires_at_utc = expires_at.with_timezone(&Utc);

                    if expires_at_utc > now {
                        let duration = expires_at_utc.signed_duration_since(now);
                        let days = duration.num_days();

                        if days > 7 {
                            println!("  {}: {} ({} days)",
                                style("Expires").dim(),
                                verify_response.expires_at,
                                days
                            );
                        } else if days > 0 {
                            println!("  {}: {} ({} days)",
                                style("Expires").dim(),
                                style(&verify_response.expires_at).yellow(),
                                style(days).yellow()
                            );
                        } else {
                            let hours = duration.num_hours();
                            println!("  {}: {} ({} hours)",
                                style("Expires").dim(),
                                style(&verify_response.expires_at).red(),
                                style(hours).red()
                            );
                        }
                    } else {
                        println!("  {}: {}",
                            style("Expires").dim(),
                            style("Expired").red()
                        );
                    }
                } else {
                    println!("  {}: {}",
                        style("Expires").dim(),
                        verify_response.expires_at
                    );
                }
            }
            Err(e) => {
                println!("  {}: {}", style("Status").dim(), style(format!("✗ {}", e)).red());
            }
        }

        println!();
    }

    Ok(())
}
