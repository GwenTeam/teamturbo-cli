use anyhow::{Context, Result};
use console::style;
use dialoguer::Input;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashSet;
use std::path::PathBuf;

use crate::api::ApiClient;
use crate::api::client::{DocumentUpdate, DocumentCreate};
use crate::config::{CliConfig, DocuramConfig};
use crate::utils::{storage::LocalState, read_file, calculate_checksum, scan_documents_with_meta, update_front_matter};

pub async fn execute(documents: Vec<String>, message: Option<String>) -> Result<()> {
    println!("{}", style("Push Document Changes").cyan().bold());
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

    // Create API client
    let client = ApiClient::new(server_url.to_string(), auth.access_token.clone());

    // Load local state
    let mut local_state = LocalState::load()?;

    // First, process documents marked for deletion
    let pending_deletions: Vec<_> = local_state
        .documents
        .values()
        .filter(|doc| doc.pending_deletion)
        .cloned()
        .collect();

    if !pending_deletions.is_empty() {
        println!("{}", style(format!("Processing {} document(s) marked for deletion...", pending_deletions.len())).cyan());
        println!();

        let mut deleted_count = 0;
        let mut failed_deletions = Vec::new();
        let mut deleted_doc_categories = Vec::new();

        for doc_info in &pending_deletions {
            match client.delete_document(&doc_info.uuid).await {
                Ok(_) => {
                    println!("  {} Deleted from server: {}", style("✓").green(), doc_info.path);

                    // Extract category path from document path
                    let doc_path = std::path::Path::new(&doc_info.path);
                    if let Some(parent) = doc_path.parent() {
                        if let Some(category_path) = parent.to_str() {
                            // Remove "docs/" prefix to get the actual category path
                            let category = category_path.strip_prefix("docs/").unwrap_or(category_path);
                            deleted_doc_categories.push(category.to_string());
                        }
                    }

                    // Remove from state.json after successful deletion
                    local_state.remove_document(&doc_info.uuid);
                    deleted_count += 1;
                }
                Err(e) => {
                    println!("  {} Failed to delete from server: {} - {}",
                        style("✗").red(), doc_info.path, e);
                    failed_deletions.push((doc_info.uuid.clone(), e.to_string()));
                }
            }
        }

        // Save state after deletions
        local_state.save()?;

        println!();
        println!("{}", style(format!("✓ {} document(s) deleted from server", deleted_count)).green().bold());
        if !failed_deletions.is_empty() {
            println!("{}", style(format!("✗ {} deletion(s) failed", failed_deletions.len())).red());
        }

        // Now check and delete empty categories
        if !deleted_doc_categories.is_empty() {
            println!();
            println!("{}", style("Checking for empty categories to delete...").cyan());

            // Get the current working category path from docuram.json to avoid deleting it
            let current_category_path = &docuram_config.docuram.category_path;

            // Get unique category paths and sort by depth (deepest first)
            let mut unique_categories: Vec<String> = deleted_doc_categories.into_iter()
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            unique_categories.sort_by(|a, b| {
                let depth_a = a.matches('/').count();
                let depth_b = b.matches('/').count();
                depth_b.cmp(&depth_a) // Sort deepest first
            });

            let mut deleted_categories = 0;

            for category_path in unique_categories {
                // Skip if this is the current working category or its parent
                if category_path == *current_category_path || current_category_path.starts_with(&format!("{}/", category_path)) {
                    continue;
                }

                // Try to delete category - server will reject if not empty
                if let Ok(Some(category_uuid)) = client.get_category_uuid_by_path(&category_path).await {
                    match client.delete_category(&category_uuid).await {
                        Ok(_) => {
                            println!("  {} Deleted empty category: {}", style("✓").green(), category_path);
                            deleted_categories += 1;
                        }
                        Err(e) => {
                            // Silently skip errors - category might not be empty or already deleted
                            // Only show error if it's not a "not empty" or "not found" error
                            let error_msg = e.to_string();
                            if !error_msg.contains("not empty") && !error_msg.contains("not found") && !error_msg.contains("Not found") {
                                println!("  {} Failed to delete category {}: {}",
                                    style("⚠").yellow(), category_path, e);
                            }
                        }
                    }
                }
            }

            if deleted_categories > 0 {
                println!("{}", style(format!("✓ {} empty categor(ies) deleted", deleted_categories)).green().bold());
            }
        }

        println!();
    }

    // Scan docs directory for new documents with front matter
    println!("{}", style("Scanning docs/ directory for new documents...").cyan());
    let new_docs_with_meta = match scan_documents_with_meta("docs") {
        Ok(docs) => docs,
        Err(_) => {
            println!("{}", style("No docs/ directory found, skipping new document scan").yellow());
            Vec::new()
        }
    };

    // Build a set of file paths from docuram.json for quick lookup
    // Only use 'documents', not 'requires' (requires are read-only dependencies)
    let docuram_paths: HashSet<String> = docuram_config
        .documents
        .iter()
        .map(|d| d.path.clone())
        .collect();

    // Build a set of file paths from state.json
    let state_paths: HashSet<String> = local_state
        .documents
        .values()
        .map(|doc_info| doc_info.path.clone())
        .collect();

    // Build a set of UUIDs from docuram.json and state.json (if document has uuid in frontmatter)
    // Only use 'documents', not 'requires'
    let docuram_uuids: HashSet<String> = docuram_config
        .documents
        .iter()
        .map(|d| d.uuid.clone())
        .collect();

    let state_uuids: HashSet<String> = local_state
        .documents
        .keys()
        .cloned()
        .collect();

    // Filter: new documents are those NOT in docuram.json AND NOT in state.json
    let new_docs: Vec<_> = new_docs_with_meta
        .into_iter()
        .filter(|d| {
            // Check if file path is in docuram.json or state.json
            let in_docuram_by_path = docuram_paths.contains(&d.file_path);
            let in_state_by_path = state_paths.contains(&d.file_path);

            // If document has UUID in frontmatter, also check by UUID
            let in_docuram_by_uuid = d.front_matter.uuid.as_ref()
                .map(|uuid| docuram_uuids.contains(uuid))
                .unwrap_or(false);
            let in_state_by_uuid = d.front_matter.uuid.as_ref()
                .map(|uuid| state_uuids.contains(uuid))
                .unwrap_or(false);

            // Document is new if it's not found by path OR uuid
            !in_docuram_by_path && !in_state_by_path && !in_docuram_by_uuid && !in_state_by_uuid
        })
        .collect();

    if !new_docs.is_empty() {
        println!("{}", style(format!("Found {} new document(s) with front matter:", new_docs.len())).bold());
        for doc in &new_docs {
            println!("  - {} ({})", doc.front_matter.title, doc.file_path);
        }
        println!();
    }

    // Determine which documents to push
    // Only push 'documents', not 'requires' (requires are read-only dependencies)
    let docs_to_check: Vec<_> = if documents.is_empty() {
        // Check all documents (only from 'documents', not 'requires')
        docuram_config.documents.iter().collect()
    } else {
        // Check specific documents
        let doc_set: HashSet<String> = documents.into_iter().collect();
        docuram_config
            .documents
            .iter()
            .filter(|doc| doc_set.contains(&doc.uuid))
            .collect()
    };

    // Check for documents in state.json that are not in docuram.json
    // These are documents that were created but docuram.json hasn't been updated
    // NOTE: Skip dependency documents (requires) - they are read-only
    let mut state_only_docs = Vec::new();
    for (uuid, doc_info) in &local_state.documents {
        // Check if this UUID is in docuram.json documents (not requires)
        let in_docuram = docuram_config
            .documents
            .iter()
            .any(|d| d.uuid == *uuid);

        // Check if this UUID is in docuram.json requires (dependencies are read-only)
        let in_requires = docuram_config
            .requires
            .iter()
            .any(|d| d.uuid == *uuid);

        if !in_docuram && !in_requires {
            // This document is in state but not in docuram.json (and not a dependency)
            let file_path = PathBuf::from(&doc_info.path);
            if file_path.exists() {
                state_only_docs.push(doc_info.clone());
            }
        }
    }

    if !docs_to_check.is_empty() || !state_only_docs.is_empty() {
        println!("Checking {} document(s) for changes...", docs_to_check.len() + state_only_docs.len());
        println!();
    } else if new_docs.is_empty() {
        println!("{}", style("No documents to push").yellow());
        return Ok(());
    }

    // Check which documents have been modified
    // Store as (uuid, title, path, content, checksum)
    let mut to_push: Vec<(String, String, String, String, String)> = Vec::new();
    let mut missing_files = Vec::new();

    // Check documents from docuram.json
    for doc_info in &docs_to_check {
        let file_path = PathBuf::from(&doc_info.path);

        if !file_path.exists() {
            missing_files.push(doc_info.uuid.clone());
            continue;
        }

        // Read current content
        let current_content = read_file(&file_path)?;
        let current_checksum = calculate_checksum(&current_content);

        // Check if modified
        let is_modified = match local_state.get_document(&doc_info.uuid) {
            Some(local_info) => current_checksum != local_info.checksum,
            None => {
                // No local state, compare with remote checksum
                current_checksum != doc_info.checksum
            }
        };

        if is_modified {
            to_push.push((
                doc_info.uuid.clone(),
                doc_info.title.clone(),
                doc_info.path.clone(),
                current_content,
                current_checksum,
            ));
        }
    }

    // Check documents from state.json that are not in docuram.json
    for state_doc in &state_only_docs {
        let file_path = PathBuf::from(&state_doc.path);

        // Read current content
        let current_content = read_file(&file_path)?;
        let current_checksum = calculate_checksum(&current_content);

        // Check if modified compared to last sync
        if current_checksum != state_doc.checksum {
            // Extract title from file path for display
            let title = file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
                .to_string();

            to_push.push((
                state_doc.uuid.clone(),
                title,
                state_doc.path.clone(),
                current_content,
                current_checksum,
            ));
        }
    }

    // Report missing files
    if !missing_files.is_empty() {
        println!("{}", style(format!("⚠ {} document(s) not found locally:", missing_files.len())).yellow());
        for uuid in &missing_files {
            println!("  - {}", uuid);
        }
        println!();
    }

    // Check if there are changes to push or new documents to create
    if to_push.is_empty() && new_docs.is_empty() {
        println!("{}", style("No changes to push").green());
        return Ok(());
    }

    // Process document updates if there are any
    let mut success_count = 0;
    let mut failed_docs = Vec::new();

    if !to_push.is_empty() {
        println!("{}", style(format!("Found {} modified document(s):", to_push.len())).bold());
        for (uuid, title, _, _, _) in &to_push {
            println!("  - {} ({})", title, uuid);
        }
        println!();

        // Get change summary
        let change_summary = match message {
            Some(msg) => msg,
            None => {
                Input::<String>::new()
                    .with_prompt("Change summary")
                    .allow_empty(true)
                    .interact_text()?
            }
        };

        let change_summary = if change_summary.trim().is_empty() {
            None
        } else {
            Some(change_summary)
        };

        println!();
        println!("{}", style(format!("Pushing {} document(s)...", to_push.len())).bold());
        println!();

        // Create progress bar
        let pb = ProgressBar::new(to_push.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
                .expect("Invalid progress bar template")
                .progress_chars("=> ")
        );

        for (uuid, title, path, content, checksum) in to_push {
            pb.set_message(format!("{}", title));

            // Push complete content including frontmatter
            // Backend will store it as-is, frontend will hide frontmatter during preview
            let update = DocumentUpdate {
                content: content.clone(),
                change_summary: change_summary.clone(),
            };

            match client.upload_document(&uuid, update).await {
                Ok(updated_doc) => {
                    // Use the version returned from server
                    let version = updated_doc.version;

                    // Update local state with server version
                    local_state.upsert_document(crate::utils::storage::LocalDocumentInfo {
                        uuid: uuid.clone(),
                        path: path.clone(),
                        checksum,
                        version,
                        last_sync: chrono::Utc::now().to_rfc3339(),
                        pending_deletion: false,
                    });
                    success_count += 1;
                }
                Err(e) => {
                    failed_docs.push((uuid.clone(), e.to_string()));
                }
            }

            pb.inc(1);
        }

        pb.finish_with_message("Done");
    }

    // Process new documents
    let mut created_count = 0;
    let mut failed_new_docs = Vec::new();

    if !new_docs.is_empty() {
        println!();
        println!("{}", style(format!("Creating {} new document(s)...", new_docs.len())).bold());
        println!();

        let pb_new = ProgressBar::new(new_docs.len() as u64);
        pb_new.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
                .expect("Invalid progress bar template")
                .progress_chars("=> ")
        );

        for new_doc in new_docs {
            pb_new.set_message(format!("{}", new_doc.front_matter.title));

            // Get or create category by path
            let category_id = match client.get_category_by_path(&new_doc.front_matter.category).await {
                Ok(Some(id)) => id,
                Ok(None) => {
                    // Category doesn't exist, create it automatically
                    match client.ensure_category_by_path(&new_doc.front_matter.category).await {
                        Ok(id) => id,
                        Err(e) => {
                            failed_new_docs.push((
                                new_doc.front_matter.title.clone(),
                                format!("Failed to create category '{}': {}", new_doc.front_matter.category, e),
                            ));
                            pb_new.inc(1);
                            continue;
                        }
                    }
                }
                Err(e) => {
                    failed_new_docs.push((new_doc.front_matter.title.clone(), e.to_string()));
                    pb_new.inc(1);
                    continue;
                }
            };

            // Create document - push complete content including frontmatter
            // Note: new_doc.content already excludes frontmatter from extract_front_matter
            // We need to reconstruct the full document with frontmatter
            let full_content = {
                use crate::utils::FrontMatterWrapper;
                let wrapper = FrontMatterWrapper {
                    docuram: new_doc.front_matter.clone(),
                };
                let yaml = serde_yaml::to_string(&wrapper).unwrap_or_default();
                format!("---\n{}---\n\n{}", yaml, new_doc.content)
            };

            let doc_create = DocumentCreate {
                category_id,
                title: new_doc.front_matter.title.clone(),
                content: full_content.clone(),
                description: new_doc.front_matter.description.clone(),
                doc_type: new_doc.front_matter.doc_type.clone().or(Some("knowledge".to_string())),
                priority: new_doc.front_matter.priority.or(Some(0)),
                is_required: None,
            };

            match client.create_document(doc_create).await {
                Ok(created_doc) => {
                    // Update the front matter with uuid, version, and category_uuid from server
                    let mut updated_front_matter = new_doc.front_matter.clone();
                    updated_front_matter.uuid = Some(created_doc.uuid.clone());
                    updated_front_matter.version = Some(created_doc.version);

                    // Get category_uuid from the response if available
                    if let Some(ref category) = created_doc.category {
                        updated_front_matter.category_uuid = Some(category.uuid.clone());
                    }

                    // Update the file with new front matter
                    if let Err(e) = update_front_matter(&new_doc.file_path, &updated_front_matter, &new_doc.content) {
                        eprintln!("Warning: Failed to update front matter for {}: {}", new_doc.file_path, e);
                    }

                    // Read the updated file content for checksum calculation
                    let updated_full_content = match read_file(&new_doc.file_path) {
                        Ok(content) => content,
                        Err(_) => {
                            // Fallback: reconstruct from updated frontmatter
                            let wrapper = crate::utils::FrontMatterWrapper {
                                docuram: updated_front_matter.clone(),
                            };
                            let yaml = serde_yaml::to_string(&wrapper).unwrap_or_default();
                            format!("---\n{}---\n\n{}", yaml, new_doc.content)
                        }
                    };

                    // Calculate checksum for local state (with complete content including frontmatter)
                    let checksum = calculate_checksum(&updated_full_content);

                    // Update local state
                    local_state.upsert_document(crate::utils::storage::LocalDocumentInfo {
                        uuid: created_doc.uuid.clone(),
                        path: new_doc.file_path.clone(),
                        checksum,
                        version: created_doc.version,
                        last_sync: chrono::Utc::now().to_rfc3339(),
                        pending_deletion: false,
                    });

                    created_count += 1;
                }
                Err(e) => {
                    failed_new_docs.push((new_doc.front_matter.title.clone(), e.to_string()));
                }
            }

            pb_new.inc(1);
        }

        pb_new.finish_with_message("Done");
    }

    // Save local state
    local_state.save()
        .context("Failed to save local state")?;

    // If we created new documents, update docuram.json from server
    if created_count > 0 {
        println!();
        println!("{}", style("Updating docuram.json from server...").cyan());

        // Get category UUID from docuram config
        let category_uuid = match &docuram_config.docuram.category_uuid {
            Some(uuid) => uuid.clone(),
            None => {
                println!("{}", style("Warning: No category UUID in docuram.json, skipping config update").yellow());
                String::new()
            }
        };

        if !category_uuid.is_empty() {
            // Fetch updated config from server
            let config_url = format!("{}/api/docuram/categories/{}/generate_config",
                server_url, category_uuid);

            match client.get_docuram_config(&config_url).await {
                Ok(updated_config) => {
                    // Save updated config
                    if let Err(e) = updated_config.save() {
                        println!("{}", style(format!("Warning: Failed to save updated docuram.json: {}", e)).yellow());
                    } else {
                        println!("{}", style("✓ Updated docuram.json").green());
                    }
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    if error_msg.contains("not found") || error_msg.contains("Not found") || error_msg.contains("404") {
                        // Category UUID is stale, try to refresh it using category path
                        println!("{}", style("Category UUID is stale, attempting to refresh...").yellow());

                        let category_path = &docuram_config.docuram.category_path;
                        match client.get_category_uuid_by_path(category_path).await {
                            Ok(Some(new_uuid)) => {
                                println!("{}", style(format!("Found new UUID for category: {}", category_path)).dim());

                                // Retry with the new UUID
                                let new_config_url = format!("{}/api/docuram/categories/{}/generate_config",
                                    server_url, new_uuid);

                                match client.get_docuram_config(&new_config_url).await {
                                    Ok(updated_config) => {
                                        // Save updated config
                                        if let Err(e) = updated_config.save() {
                                            println!("{}", style(format!("Warning: Failed to save updated docuram.json: {}", e)).yellow());
                                        } else {
                                            println!("{}", style("✓ Updated docuram.json with refreshed category UUID").green());
                                        }
                                    }
                                    Err(e) => {
                                        println!("{}", style(format!("Warning: Failed to fetch config with new UUID: {}", e)).yellow());
                                        println!("{}", style("  Run 'teamturbo init' to re-initialize.").dim());
                                    }
                                }
                            }
                            Ok(None) => {
                                println!("{}", style(format!("Category '{}' not found on server. Please run 'teamturbo init' to re-initialize.", category_path)).yellow());
                            }
                            Err(e) => {
                                println!("{}", style(format!("Failed to lookup category UUID: {}", e)).yellow());
                                println!("{}", style("  Run 'teamturbo init' to re-initialize.").dim());
                            }
                        }
                    } else {
                        println!("{}", style(format!("Warning: Failed to fetch updated config: {}", e)).yellow());
                        println!("{}", style("  Run 'teamturbo pull --config' to manually update docuram.json").dim());
                    }
                }
            }
        }
    }

    println!();

    // Report results
    if failed_docs.is_empty() && created_count == 0 {
        println!("{}", style(format!("✓ Successfully pushed {} document(s)", success_count)).green());
    } else {
        if success_count > 0 {
            println!("{}", style(format!("✓ Updated {} document(s)", success_count)).green());
        }
        if created_count > 0 {
            println!("{}", style(format!("✓ Created {} new document(s)", created_count)).green());
        }
        if !failed_docs.is_empty() {
            println!("{}", style(format!("✗ Failed to update {} document(s):", failed_docs.len())).red());
            for (uuid, error) in failed_docs {
                println!("  - {}: {}", uuid, error);
            }
        }
        if !failed_new_docs.is_empty() {
            println!("{}", style(format!("✗ Failed to create {} document(s):", failed_new_docs.len())).red());
            for (title, error) in failed_new_docs {
                println!("  - {}: {}", title, error);
            }
        }
    }

    Ok(())
}

/// Remove docuram metadata frontmatter from content before uploading
fn remove_docuram_metadata(content: &str) -> String {
    // Check if content starts with docuram frontmatter
    if content.starts_with("---\ndocuram:") || content.starts_with("---\r\ndocuram:") {
        // Find the end of frontmatter (second occurrence of "---")
        let lines: Vec<&str> = content.lines().collect();
        let mut end_index = 0;
        let mut found_start = false;

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed == "---" {
                if found_start {
                    // Found the closing "---"
                    end_index = i + 1;
                    break;
                } else {
                    // Found the opening "---"
                    found_start = true;
                }
            }
        }

        if end_index > 0 && end_index < lines.len() {
            // Return content after frontmatter, skipping any leading empty lines
            let remaining = lines[end_index..].join("\n");
            return remaining.trim_start().to_string();
        }
    }

    // No frontmatter found or couldn't parse, return original
    content.to_string()
}
