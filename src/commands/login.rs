use anyhow::Result;
use console::style;
use dialoguer::Input;
use crate::auth;
use crate::config::CliConfig;

/// Parse domain input and convert to full URL
/// - If input starts with http:// or https://, use as-is
/// - Otherwise, treat as subdomain and construct https://{subdomain}.teamturbo.io
fn parse_domain(domain: &str) -> String {
    let domain = domain.trim().trim_end_matches('/');

    if domain.starts_with("http://") || domain.starts_with("https://") {
        domain.to_string()
    } else {
        // Treat as subdomain
        format!("https://{}.teamturbo.io", domain)
    }
}

pub async fn execute(domain: Option<String>, _force_browser: bool, force_manual: bool) -> Result<()> {
    println!("{}", style("TeamTurbo CLI Login").cyan().bold());
    println!();

    // Get server URL
    let server_url: String = if let Some(domain_input) = domain {
        // Use provided domain parameter
        parse_domain(&domain_input)
    } else {
        // Interactive prompt
        let input: String = Input::new()
            .with_prompt("Server domain or URL")
            .default("example".to_string())
            .interact_text()?;
        parse_domain(&input)
    };

    println!("{} {}", style("→ Connecting to:").dim(), style(&server_url).cyan());
    println!();

    // Determine authentication mode
    let use_browser = if force_manual {
        false
    } else {
        // Default to browser mode (unless force_manual is set)
        true
    };

    // Perform authorization
    let auth_config = if use_browser {
        auth::browser::authorize(&server_url).await?
    } else {
        auth::manual::authorize(&server_url).await?
    };

    // Save to config
    let mut config = CliConfig::load()?;
    config.set_auth(server_url.clone(), auth_config);
    config.save()?;

    println!();
    println!("{}", style("✓ Token saved to ~/.teamturbo-cli/config.toml").green());
    println!();
    println!("{}", style("You can now use other commands like:").dim());
    println!("  {} {}", style("teamturbo init --config-url").dim(), style("<config_url>").yellow());
    println!("  {} {}", style("teamturbo pull").dim(), style("").yellow());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_domain_subdomain() {
        assert_eq!(parse_domain("example"), "https://example.teamturbo.io");
        assert_eq!(parse_domain("my-team"), "https://my-team.teamturbo.io");
        assert_eq!(parse_domain("test123"), "https://test123.teamturbo.io");
    }

    #[test]
    fn test_parse_domain_https() {
        assert_eq!(parse_domain("https://example.com"), "https://example.com");
        assert_eq!(parse_domain("https://api.example.com"), "https://api.example.com");
        assert_eq!(parse_domain("https://example.com/"), "https://example.com");
    }

    #[test]
    fn test_parse_domain_http() {
        assert_eq!(parse_domain("http://localhost:3000"), "http://localhost:3000");
        assert_eq!(parse_domain("http://192.168.1.100"), "http://192.168.1.100");
        assert_eq!(parse_domain("http://example.com/"), "http://example.com");
    }

    #[test]
    fn test_parse_domain_whitespace() {
        assert_eq!(parse_domain("  example  "), "https://example.teamturbo.io");
        assert_eq!(parse_domain(" https://example.com/ "), "https://example.com");
    }
}
