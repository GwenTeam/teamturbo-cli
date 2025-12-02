use anyhow::Result;
use console::style;
use crate::config::CliConfig;
use crate::api::ApiClient;

pub async fn execute() -> Result<()> {
    println!("{}", style("TeamTurbo CLI Logout").cyan().bold());
    println!();

    // Load config
    let mut config = CliConfig::load()?;

    // Check if there are any saved auth configs
    if config.auth.is_empty() {
        println!("{}", style("Not logged in to any server").yellow());
        return Ok(());
    }

    // Show logged in servers
    println!("Currently logged in to:");
    for (i, (server, _)) in config.auth.iter().enumerate() {
        println!("  {}. {}", i + 1, server);
    }
    println!();

    // Logout from all servers
    let mut success_count = 0;
    let mut failed_servers = Vec::new();

    for (server_url, auth_config) in config.auth.iter() {
        print!("Logging out from {}... ", server_url);

        let client = ApiClient::new(server_url.clone(), auth_config.access_token.clone());

        match client.logout().await {
            Ok(_) => {
                println!("{}", style("✓").green());
                success_count += 1;
            }
            Err(e) => {
                println!("{}", style(format!("✗ {}", e)).red());
                failed_servers.push(server_url.clone());
            }
        }
    }

    // Clear all auth configs from local file
    config.auth.clear();
    config.save()?;

    println!();
    if failed_servers.is_empty() {
        println!("{}", style(format!("✓ Logged out from {} server(s)", success_count)).green());
    } else {
        println!("{}", style(format!("✓ Logged out from {} server(s)", success_count)).green());
        println!("{}", style(format!("⚠ Failed to revoke tokens on {} server(s)", failed_servers.len())).yellow());
        println!("{}", style("(Local credentials have been cleared)").dim());
    }

    println!();
    println!("{}", style("All local credentials have been removed").dim());

    Ok(())
}
