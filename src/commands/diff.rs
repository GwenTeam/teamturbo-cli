use anyhow::{Context, Result};
use console::style;
use std::path::PathBuf;

use crate::api::ApiClient;
use crate::config::{CliConfig, DocuramConfig};
use crate::utils::{storage::LocalState, read_file, calculate_checksum};

pub async fn execute(document: Option<String>) -> Result<()> {
    println!("{}", style("Document Diff").cyan().bold());
    println!();

    // Load docuram config
    let docuram_config = DocuramConfig::load()
        .context("Failed to load docuram.json. Run 'teamturbo init' first.")?;

    // Load CLI config
    let cli_config = CliConfig::load()?;

    let server_url = docuram_config.server_url();

    // Get auth for this server
    let auth = cli_config
        .get_auth(server_url)
        .context(format!("Not logged in to {}. Run 'teamturbo login' first.", server_url))?;

    // Create API client (unused for now, but needed for future remote diff)
    let _client = ApiClient::new(server_url.to_string(), auth.access_token.clone());

    // Load local state
    let local_state = LocalState::load()?;

    // Determine which documents to check
    let docs_to_check: Vec<_> = if let Some(uuid) = document {
        // Check specific document
        docuram_config
            .all_documents()
            .filter(|doc| doc.uuid == uuid)
            .collect()
    } else {
        // Check all documents
        docuram_config.all_documents().collect()
    };

    if docs_to_check.is_empty() {
        println!("{}", style("No documents found").yellow());
        return Ok(());
    }

    println!("Checking {} document(s)...", docs_to_check.len());
    println!();

    // Check each document
    let mut modified_count = 0;
    let mut untracked_count = 0;
    let mut missing_count = 0;
    let mut up_to_date_count = 0;

    for doc_info in &docs_to_check {
        // Use local_path() to get correct path (dependencies go in working_category/dependencies/ subdirectory)
        let working_category_path = &docuram_config.docuram.category_path;
        let local_file_path = doc_info.local_path(working_category_path);
        let file_path = PathBuf::from(&local_file_path);

        if !file_path.exists() {
            println!("{} {} {}",
                style("missing:").red().bold(),
                style(&doc_info.uuid).red(),
                style(format!("({})", doc_info.title)).dim()
            );
            missing_count += 1;
            continue;
        }

        // Read current content
        let current_content = match read_file(&file_path) {
            Ok(content) => content,
            Err(e) => {
                println!("{} {} {}",
                    style("error:").red().bold(),
                    style(&doc_info.uuid).red(),
                    style(format!("({})", e)).dim()
                );
                continue;
            }
        };

        let current_checksum = calculate_checksum(&current_content);

        // Check status
        match local_state.get_document(&doc_info.uuid) {
            Some(local_info) => {
                if current_checksum != local_info.checksum {
                    // Modified since last sync
                    println!("{} {} {}",
                        style("modified:").yellow().bold(),
                        style(&doc_info.uuid).yellow(),
                        style(format!("({})", doc_info.title)).dim()
                    );
                    modified_count += 1;

                    // Show line count diff
                    let _old_lines = local_info.checksum.len(); // Placeholder for future use
                    let new_lines = current_content.lines().count();
                    println!("  {} {} lines",
                        style("→").dim(),
                        style(format!("{}", new_lines)).cyan()
                    );
                } else if current_checksum != doc_info.checksum {
                    // Local matches state but remote is different
                    println!("{} {} {}",
                        style("outdated:").cyan().bold(),
                        style(&doc_info.uuid).cyan(),
                        style(format!("({})", doc_info.title)).dim()
                    );
                    println!("  {} Remote has updates available",
                        style("→").dim()
                    );
                    up_to_date_count += 1;
                } else {
                    // Up to date
                    if docs_to_check.len() == 1 {
                        // Only show if checking single document
                        println!("{} {} {}",
                            style("clean:").green().bold(),
                            style(&doc_info.uuid).green(),
                            style(format!("({})", doc_info.title)).dim()
                        );
                    }
                    up_to_date_count += 1;
                }
            }
            None => {
                // No local state, untracked
                println!("{} {} {}",
                    style("untracked:").magenta().bold(),
                    style(&doc_info.uuid).magenta(),
                    style(format!("({})", doc_info.title)).dim()
                );
                untracked_count += 1;
            }
        }
    }

    println!();
    println!("{}", style("Summary:").bold());
    if modified_count > 0 {
        println!("  {} {} document(s) modified",
            style("●").yellow(),
            style(modified_count).yellow()
        );
    }
    if untracked_count > 0 {
        println!("  {} {} document(s) untracked",
            style("●").magenta(),
            style(untracked_count).magenta()
        );
    }
    if missing_count > 0 {
        println!("  {} {} document(s) missing",
            style("●").red(),
            style(missing_count).red()
        );
    }
    if up_to_date_count > 0 {
        println!("  {} {} document(s) up to date",
            style("●").green(),
            style(up_to_date_count).green()
        );
    }

    println!();
    if modified_count > 0 || untracked_count > 0 {
        println!("{}", style("Use 'teamturbo push' to upload changes").dim());
    }
    if missing_count > 0 {
        println!("{}", style("Use 'teamturbo pull' to download missing documents").dim());
    }

    Ok(())
}
