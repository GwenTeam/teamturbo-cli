use anyhow::{Context, Result};
use console::style;
use regex::Regex;

use crate::api::ApiClient;
use crate::config::{CliConfig, DocuramConfig};
use crate::utils::logger;

/// Execute feedback command
pub async fn execute(targets: Vec<String>, message: String, verbose: bool) -> Result<()> {
    println!("{}", style("Send Feedback").cyan().bold());
    println!();

    // Validate inputs
    validate_inputs(&targets, &message)?;

    // Load docuram config
    let docuram_config = DocuramConfig::load()
        .context("Failed to load docuram/docuram.json. Run 'teamturbo init' first.")?;

    // Load CLI config
    let cli_config = CliConfig::load()
        .context("Failed to load configuration. Run 'teamturbo login' first.")?;

    // Get server URL from docuram config
    let server_url = docuram_config.server_url();

    // Get auth for this server
    let auth = cli_config
        .get_auth(server_url)
        .context(format!("Not logged in to {}. Run 'teamturbo login' first.", server_url))?;

    // Create API client
    let client = ApiClient::new(server_url.to_string(), auth.access_token.clone());

    if verbose {
        println!("{}:", style("Request").cyan());
        println!("  Target UUIDs: {:?}", targets);
        println!("  Message: \"{}\"", message);
        println!();
    }

    // Send feedback
    println!("Sending feedback...");
    
    let response = client
        .send_feedback(targets, message)
        .await
        .context("Failed to send feedback")?;

    if verbose {
        println!();
        println!("{}:", style("Response").cyan());
        println!("  Status: {}", style("200 OK").green());
        println!("  Recipients: {}", response.recipients.len());
    }

    println!();
    println!("{}", style("✓ Feedback sent successfully").green().bold());

    if !response.recipients.is_empty() {
        println!();
        println!("{}:", style("Recipients").bold());
        for recipient in &response.recipients {
            println!("  • {} ({})", recipient.user_name, recipient.email);
        }

        let count = response.recipients.len();
        if count > 1 {
            println!(
                "\n{}",
                style(format!("Your feedback has been delivered to {} recipients.", count))
                    .green()
            );
        } else {
            println!("\n{}", style("Your feedback has been delivered.").green());
        }
    }

    Ok(())
}

/// Validate input parameters
fn validate_inputs(targets: &[String], message: &str) -> Result<()> {
    // Validate UUIDs
    let uuid_regex = Regex::new(
        r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
    )
    .context("Failed to compile UUID regex")?;

    for uuid in targets {
        if !uuid_regex.is_match(uuid) {
            anyhow::bail!(
                "Invalid UUID format: {}\n\nExpected format: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
                uuid
            );
        }
    }

    // Validate target count
    if targets.is_empty() {
        anyhow::bail!("At least one target UUID is required.\n\nUsage:\n  teamturbo feedback <uuid> --message \"Your message\"");
    }

    if targets.len() > 10 {
        anyhow::bail!("Too many targets specified (maximum 10, got {}).", targets.len());
    }

    // Validate message
    let trimmed = message.trim();
    if trimmed.is_empty() {
        anyhow::bail!("Message cannot be empty.\n\nUsage:\n  teamturbo feedback <uuid> --message \"Your message\"");
    }

    if trimmed.len() > 2000 {
        anyhow::bail!(
            "Message is too long ({} characters). Maximum length is 2000 characters.",
            trimmed.len()
        );
    }

    logger::debug(
        "validate_inputs",
        &format!(
            "Validated {} target(s) and message ({} chars)",
            targets.len(),
            trimmed.len()
        ),
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_uuid_format_valid() {
        let targets = vec!["12345678-1234-1234-1234-123456789abc".to_string()];
        let message = "Test message".to_string();
        assert!(validate_inputs(&targets, &message).is_ok());
    }

    #[test]
    fn test_validate_uuid_format_invalid() {
        let targets = vec!["invalid-uuid".to_string()];
        let message = "Test message".to_string();
        assert!(validate_inputs(&targets, &message).is_err());
    }

    #[test]
    fn test_validate_empty_message() {
        let targets = vec!["12345678-1234-1234-1234-123456789abc".to_string()];
        let message = "   ".to_string();
        assert!(validate_inputs(&targets, &message).is_err());
    }

    #[test]
    fn test_validate_message_too_long() {
        let targets = vec!["12345678-1234-1234-1234-123456789abc".to_string()];
        let message = "a".repeat(2001);
        assert!(validate_inputs(&targets, &message).is_err());
    }

    #[test]
    fn test_validate_too_many_targets() {
        let targets = vec!["12345678-1234-1234-1234-123456789abc".to_string(); 11];
        let message = "Test message".to_string();
        assert!(validate_inputs(&targets, &message).is_err());
    }

    #[test]
    fn test_validate_empty_targets() {
        let targets = vec![];
        let message = "Test message".to_string();
        assert!(validate_inputs(&targets, &message).is_err());
    }

    #[test]
    fn test_validate_multiple_valid_uuids() {
        let targets = vec![
            "12345678-1234-1234-1234-123456789abc".to_string(),
            "87654321-4321-4321-4321-cba987654321".to_string(),
        ];
        let message = "This is a valid message".to_string();
        assert!(validate_inputs(&targets, &message).is_ok());
    }

    #[test]
    fn test_validate_case_insensitive_uuid() {
        let targets = vec!["ABCDEF12-ABCD-ABCD-ABCD-ABCDEFABCDEF".to_string()];
        let message = "Test message".to_string();
        assert!(validate_inputs(&targets, &message).is_ok());
    }
}
