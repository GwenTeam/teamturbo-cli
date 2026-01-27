use anyhow::{Context, Result};
use console::style;
use dialoguer::Input;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashSet;
use std::path::PathBuf;
use walkdir::WalkDir;

use crate::api::ApiClient;
use crate::api::client::{DocumentUpdate, DocumentCreate};
use crate::config::{CliConfig, DocuramConfig, DocumentInfo};
use crate::utils::{read_file, calculate_checksum};

/// Simple struct representing a new document (no frontmatter)
struct NewDocument {
    file_path: String,
    content: String,
    title: String,
}

/// Scan docuram/ directory for markdown files
fn scan_markdown_files(dir: &str) -> Result<Vec<NewDocument>> {
    let mut documents = Vec::new();

    for entry in WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Skip hidden files and directories
        if path.file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with('.'))
            .unwrap_or(false)
        {
            continue;
        }

        // Only process .md files
        if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }

        // Read file content
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Extract title from filename (with .md extension for server)
        let filename = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled");
        let title = format!("{}.md", filename);

        documents.push(NewDocument {
            file_path: path.to_string_lossy().to_string(),
            content,
            title,
        });
    }

    Ok(documents)
}

pub async fn execute(documents: Vec<String>, message: Option<String>) -> Result<()> {
    println!("{}", style("Push Document Changes").cyan().bold());
    println!();

    // Load docuram config with migration from state.json
    let mut docuram_config = DocuramConfig::load_with_migration()
        .context("Failed to load docuram.json. Run 'teamturbo init' first.")?;

    // Load CLI config
    let cli_config = CliConfig::load()?;

    let server_url = docuram_config.server_url().to_string();

    // Get auth for this server
    let auth = cli_config
        .get_auth(&server_url)
        .context(format!("Not logged in to {}. Run 'teamturbo login' first.", server_url))?;

    // Create API client
    let client = ApiClient::new(server_url.clone(), auth.access_token.clone());

    // Auto-detect missing files and mark them as pending deletion
    let mut newly_marked_count = 0;
    let working_category_path_for_check = docuram_config.docuram.category_path.clone();
    for doc in docuram_config.all_documents_mut() {
        if !doc.pending_deletion && doc.local_checksum.is_some() {
            // Get the correct local path for this document
            let local_file_path = doc.local_path(&working_category_path_for_check);
            let file_path = std::path::Path::new(&local_file_path);
            if !file_path.exists() {
                doc.pending_deletion = true;
                newly_marked_count += 1;
            }
        }
    }

    if newly_marked_count > 0 {
        println!("{}", style(format!("Detected {} missing file(s), marked for deletion", newly_marked_count)).yellow());
        docuram_config.save()?;
    }

    // First, process documents marked for deletion
    let pending_deletions: Vec<_> = docuram_config.get_pending_deletions()
        .into_iter()
        .map(|d| (d.uuid.clone(), d.path.clone()))
        .collect();

    if !pending_deletions.is_empty() {
        println!("{}", style(format!("Processing {} document(s) marked for deletion...", pending_deletions.len())).cyan());
        println!();

        let mut deleted_count = 0;
        let mut deleted_uuids = Vec::new();
        let mut failed_deletions = Vec::new();

        for (uuid, path) in &pending_deletions {
            match client.delete_document(uuid).await {
                Ok(_) => {
                    println!("  {} Deleted from server: {}", style("✓").green(), path);
                    deleted_uuids.push(uuid.clone());
                    deleted_count += 1;
                }
                Err(e) => {
                    println!("  {} Failed to delete from server: {} - {}",
                        style("✗").red(), path, e);
                    failed_deletions.push((uuid.clone(), e.to_string()));
                }
            }
        }

        // Remove deleted documents from docuram.json
        if !deleted_uuids.is_empty() {
            for uuid in &deleted_uuids {
                docuram_config.remove_document_by_uuid(uuid);
            }
            docuram_config.save()?;
        }

        println!();
        println!("{}", style(format!("✓ {} document(s) deleted from server", deleted_count)).green().bold());
        if !failed_deletions.is_empty() {
            println!("{}", style(format!("✗ {} deletion(s) failed", failed_deletions.len())).red());
        }

        println!();
    }

    // Scan docuram directory for new documents (by comparing files vs JSON)
    println!("{}", style("Scanning docuram/ directory for new documents...").cyan());
    let all_md_files = match scan_markdown_files("docuram") {
        Ok(docs) => docs,
        Err(_) => {
            println!("{}", style("No docuram/ directory found, skipping new document scan").yellow());
            Vec::new()
        }
    };

    // Get working category path for local_path() conversion
    let working_category_path = &docuram_config.docuram.category_path;

    // Build a set of LOCAL file paths from docuram.json for quick lookup
    // Use local_path() to convert server paths to local file system paths
    let docuram_paths: HashSet<String> = docuram_config
        .all_documents()
        .map(|d| d.local_path(working_category_path))
        .collect();

    // Build a set of file paths from local_documents
    let local_doc_paths: HashSet<String> = docuram_config
        .local_documents
        .iter()
        .map(|d| d.path.clone())
        .collect();

    // Filter: new documents are those NOT in docuram.json AND NOT in local_documents (by path)
    // Also exclude documents in dependencies/ directory (they are read-only)
    let new_docs: Vec<_> = all_md_files
        .into_iter()
        .filter(|d| {
            // Exclude documents in dependencies/ directory (at project root)
            if d.file_path.starts_with("dependencies/") {
                return false;
            }

            // Check if file path is in docuram.json or local_documents
            let in_docuram = docuram_paths.contains(&d.file_path);
            let in_local = local_doc_paths.contains(&d.file_path);

            // Document is new if not found by path
            !in_docuram && !in_local
        })
        .collect();

    if !new_docs.is_empty() {
        println!("{}", style(format!("Found {} new document(s):", new_docs.len())).bold());
        for doc in &new_docs {
            println!("  - {} ({})", doc.title, doc.file_path);
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

    if !docs_to_check.is_empty() {
        println!("Checking {} document(s) for changes...", docs_to_check.len());
        println!();
    } else if new_docs.is_empty() {
        println!("{}", style("No documents to push").yellow());
        return Ok(());
    }

    // Check which documents have been modified
    // Store as (uuid, title, path, content, checksum)
    let mut to_push: Vec<(String, String, String, String, String)> = Vec::new();
    let mut missing_files = Vec::new();

    // Check documents from docuram.json (only 'documents', not 'requires')
    for doc_info in &docs_to_check {
        // Use local_path() to get correct path (dependencies go in working_category/dependencies/ subdirectory)
        let working_category_path = &docuram_config.docuram.category_path;
        let local_file_path = doc_info.local_path(working_category_path);
        let file_path = PathBuf::from(&local_file_path);

        if !file_path.exists() {
            missing_files.push(doc_info.uuid.clone());
            continue;
        }

        // Read current content
        let current_content = read_file(&file_path)?;
        let current_checksum = calculate_checksum(&current_content);

        // Check if modified by comparing with local_checksum (from last sync)
        let is_modified = match &doc_info.local_checksum {
            Some(local_cs) => current_checksum != *local_cs,
            None => {
                // No local checksum, compare with remote checksum
                current_checksum != doc_info.checksum
            }
        };

        if is_modified {
            to_push.push((
                doc_info.uuid.clone(),
                doc_info.title.clone(),
                local_file_path,  // Use the local path we already computed
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
                    // Update document's local state in docuram config
                    if let Some(doc_mut) = docuram_config.get_document_by_uuid_mut(&uuid) {
                        doc_mut.local_checksum = Some(checksum.clone());
                        doc_mut.last_sync = Some(chrono::Utc::now().to_rfc3339());
                        doc_mut.version = updated_doc.version;
                    }
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

        // Get working category path from docuram config
        let working_category_path = &docuram_config.docuram.category_path;

        for new_doc in new_docs {
            pb_new.set_message(format!("{}", new_doc.title));

            // Infer correct category path based on file location
            // If the file is in docuram/organic/, docuram/impl/, docuram/req/, or docuram/manual/,
            // we need to ensure the category is set to <working_category>/<subdir> and preserve subdirectories
            let category_path = if let Some(stripped) = new_doc.file_path.strip_prefix("docuram/") {
                // Extract the directory path (without the filename)
                let path = std::path::Path::new(stripped);
                if let Some(parent) = path.parent() {
                    let parent_str = parent.to_string_lossy();
                    if parent_str.starts_with("organic") ||
                       parent_str.starts_with("impl") ||
                       parent_str.starts_with("req") ||
                       parent_str.starts_with("manual") {
                        // Standard docuram directory, prepend working category path
                        format!("{}/{}", working_category_path, parent_str)
                    } else {
                        // Other directory, use working category path
                        working_category_path.to_string()
                    }
                } else {
                    // File at root of docuram/, use working category path
                    working_category_path.to_string()
                }
            } else {
                // Not in docuram/ directory, use working category path
                working_category_path.to_string()
            };

            // Get or create category by path
            let category_id = match client.get_category_by_path(&category_path).await {
                Ok(Some(id)) => id,
                Ok(None) => {
                    // Category doesn't exist, create it automatically
                    match client.ensure_category_by_path(&category_path).await {
                        Ok(id) => id,
                        Err(e) => {
                            failed_new_docs.push((
                                new_doc.title.clone(),
                                format!("Failed to create category '{}': {}", category_path, e),
                            ));
                            pb_new.inc(1);
                            continue;
                        }
                    }
                }
                Err(e) => {
                    failed_new_docs.push((new_doc.title.clone(), e.to_string()));
                    pb_new.inc(1);
                    continue;
                }
            };

            // Create document - push pure markdown content
            let doc_create = DocumentCreate {
                category_id,
                title: new_doc.title.clone(),
                content: new_doc.content.clone(),
                description: None,
                doc_type: Some("knowledge".to_string()),
                priority: Some(0),
                is_required: None,
            };

            match client.create_document(doc_create).await {
                Ok(created_doc) => {
                    // Calculate checksum for local state
                    let checksum = calculate_checksum(&new_doc.content);

                    // Extract category info from created document
                    let (cat_name, cat_uuid) = created_doc.category
                        .as_ref()
                        .map(|c| (c.name.clone(), c.uuid.clone()))
                        .unwrap_or_else(|| (
                            String::new(),
                            docuram_config.docuram.category_uuid.clone().unwrap_or_default()
                        ));

                    // Add new document to docuram_config.documents
                    let new_doc_info = DocumentInfo {
                        id: created_doc.id,
                        uuid: created_doc.uuid.clone(),
                        title: created_doc.title.clone(),
                        category_id: category_id,
                        category_name: cat_name,
                        category_path: category_path.clone(),
                        category_uuid: cat_uuid,
                        doc_type: created_doc.doc_type.clone(),
                        version: created_doc.version,
                        path: new_doc.file_path.clone(),
                        checksum: checksum.clone(),
                        is_required: false,
                        // Local state fields
                        local_checksum: Some(checksum),
                        last_sync: Some(chrono::Utc::now().to_rfc3339()),
                        pending_deletion: false,
                    };

                    docuram_config.documents.push(new_doc_info);

                    // Remove from local_documents if it was there
                    docuram_config.local_documents.retain(|d| d.path != new_doc.file_path);

                    created_count += 1;
                }
                Err(e) => {
                    failed_new_docs.push((new_doc.title.clone(), e.to_string()));
                }
            }

            pb_new.inc(1);
        }

        pb_new.finish_with_message("Done");
    }

    // Save docuram config with updated local state
    docuram_config.save()
        .context("Failed to save docuram.json")?;

    // If we created new documents, update docuram.json from server
    // But preserve local state fields (local_checksum, last_sync, pending_deletion)
    if created_count > 0 {
        println!();
        println!("{}", style("Updating docuram.json from server...").cyan());

        // Save local state before fetching server config
        // Map: uuid -> (local_checksum, last_sync, pending_deletion)
        let local_state_backup: std::collections::HashMap<String, (Option<String>, Option<String>, bool)> =
            docuram_config.all_documents()
                .map(|d| (d.uuid.clone(), (d.local_checksum.clone(), d.last_sync.clone(), d.pending_deletion)))
                .collect();

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
                    // Save server config first
                    if let Err(e) = updated_config.save() {
                        println!("{}", style(format!("Warning: Failed to save updated docuram.json: {}", e)).yellow());
                    } else {
                        // Reload and restore local state
                        if let Ok(mut reloaded_config) = DocuramConfig::load() {
                            restore_local_state(&mut reloaded_config, &local_state_backup);
                            if let Err(e) = reloaded_config.save() {
                                println!("{}", style(format!("Warning: Failed to save local state: {}", e)).yellow());
                            }
                        }
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
                                        // Save server config first
                                        if let Err(e) = updated_config.save() {
                                            println!("{}", style(format!("Warning: Failed to save updated docuram.json: {}", e)).yellow());
                                        } else {
                                            // Reload and restore local state
                                            if let Ok(mut reloaded_config) = DocuramConfig::load() {
                                                restore_local_state(&mut reloaded_config, &local_state_backup);
                                                if let Err(e) = reloaded_config.save() {
                                                    println!("{}", style(format!("Warning: Failed to save local state: {}", e)).yellow());
                                                }
                                            }
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

/// Restore local state fields to a config after server update
fn restore_local_state(
    config: &mut DocuramConfig,
    backup: &std::collections::HashMap<String, (Option<String>, Option<String>, bool)>,
) {
    for doc in config.all_documents_mut() {
        if let Some((local_checksum, last_sync, pending_deletion)) = backup.get(&doc.uuid) {
            doc.local_checksum = local_checksum.clone();
            doc.last_sync = last_sync.clone();
            doc.pending_deletion = *pending_deletion;
        }
    }
}
