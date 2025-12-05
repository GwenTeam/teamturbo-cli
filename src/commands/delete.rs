use anyhow::{Context, Result};
use console::style;
use dialoguer::Confirm;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::DocuramConfig;
use crate::utils::storage::LocalState;
use crate::utils::{read_file, extract_front_matter};

pub async fn execute(paths: Vec<String>, force: bool, _verbose: bool) -> Result<()> {
    println!();
    println!("{}", style("Delete Documents").bold());
    println!();

    if paths.is_empty() {
        anyhow::bail!("No paths specified. Please provide at least one document or directory path.");
    }

    // Load docuram config
    let mut docuram_config = DocuramConfig::load()
        .context("Failed to load docuram.json. Make sure you're in a docuram project directory.")?;

    // Load local state
    let mut local_state = LocalState::load().unwrap_or_default();

    // Resolve paths to absolute paths and normalize
    let base_dir = std::env::current_dir()?;
    let mut target_paths: Vec<PathBuf> = Vec::new();

    for path_str in &paths {
        let path = PathBuf::from(path_str);
        let absolute_path = if path.is_absolute() {
            path
        } else {
            base_dir.join(&path)
        };

        // Accept both existing and non-existing paths
        // Non-existing paths may still have documents in docuram.json to clean up
        if !absolute_path.exists() {
            println!("{} Path does not exist locally: {}", style("ⓘ").dim(), path_str);
        }

        target_paths.push(absolute_path);
    }

    if target_paths.is_empty() {
        anyhow::bail!("No paths specified.");
    }

    // Collect all documents to delete (including documents in directories)
    let mut docs_to_delete = Vec::new();
    let mut files_to_delete = Vec::new();

    for target_path in &target_paths {
        if target_path.is_file() {
            // Single file - find matching document
            if let Some(doc) = find_document_by_path(&docuram_config, &local_state, target_path) {
                docs_to_delete.push(doc);
                files_to_delete.push(target_path.clone());
            } else {
                println!("{} No document found for: {}",
                    style("⚠").yellow(),
                    target_path.display()
                );
            }
        } else {
            // Directory or non-existent path - try to find all documents in this path
            // For non-existent paths, we check docuram.json for documents that would be under this path
            let (dir_docs, dir_files) = find_documents_in_directory(
                &docuram_config,
                &local_state,
                target_path
            );

            if dir_docs.is_empty() {
                // If still no documents found, try as a single file
                if let Some(doc) = find_document_by_path(&docuram_config, &local_state, target_path) {
                    docs_to_delete.push(doc);
                    if target_path.exists() {
                        files_to_delete.push(target_path.clone());
                    }
                } else {
                    println!("{} No document found for: {}",
                        style("⚠").yellow(),
                        target_path.display()
                    );
                }
            } else {
                docs_to_delete.extend(dir_docs);
                files_to_delete.extend(dir_files);
            }
        }
    }

    if docs_to_delete.is_empty() {
        println!("{}", style("No documents to delete.").yellow());
        return Ok(());
    }

    // Categorize documents: uploaded vs local-only vs config-only
    let mut uploaded_docs = Vec::new();
    let mut local_only_docs = Vec::new();
    let mut config_only_docs = Vec::new();

    for doc in &docs_to_delete {
        let doc_path = PathBuf::from(&doc.path);
        let file_exists = doc_path.exists();

        if local_state.documents.contains_key(&doc.uuid) {
            // Document is in state.json, meaning it was uploaded
            uploaded_docs.push(doc.clone());
        } else if file_exists {
            // Document exists locally but not in state.json (local-only)
            local_only_docs.push(doc.clone());
        } else {
            // Document is in docuram.json but file doesn't exist and not uploaded (config-only)
            config_only_docs.push(doc.clone());
        }
    }

    // Display summary
    println!("{}", style(format!("Found {} document(s) to delete:", docs_to_delete.len())).bold());
    println!();

    if !local_only_docs.is_empty() {
        println!("{}", style("Local-only documents (file exists, will be deleted):").cyan());
        for doc in &local_only_docs {
            println!("  - {} ({})", doc.title, doc.path);
        }
        println!();
    }

    if !uploaded_docs.is_empty() {
        println!("{}", style("Uploaded documents (will be deleted from both local and server):").yellow());
        for doc in &uploaded_docs {
            println!("  - {} ({})", doc.title, doc.path);
        }
        println!();
    }

    if !config_only_docs.is_empty() {
        println!("{}", style("Config-only documents (file doesn't exist, will be removed from docuram.json):").dim());
        for doc in &config_only_docs {
            println!("  - {} ({})", doc.title, doc.path);
        }
        println!();
    }

    // Confirm deletion
    if !force {
        let message = if !uploaded_docs.is_empty() {
            "This will delete files locally and mark uploaded documents for deletion (will be deleted from server on next push). Continue?"
        } else if !local_only_docs.is_empty() {
            "This will delete local files. Continue?"
        } else {
            "This will remove documents from docuram.json. Continue?"
        };

        let confirmed = Confirm::new()
            .with_prompt(message)
            .default(false)
            .interact()?;

        if !confirmed {
            println!();
            println!("{}", style("Deletion cancelled.").yellow());
            return Ok(());
        }
    }

    println!();
    println!("{}", style("Deleting documents locally...").bold());
    println!();

    // Mark uploaded and config-only documents as pending deletion in state.json
    let server_docs: Vec<_> = uploaded_docs.iter().chain(config_only_docs.iter()).collect();
    if !server_docs.is_empty() {
        println!("{}", style("Marking documents for deletion from server...").dim());

        for doc in &server_docs {
            if let Some(doc_info) = local_state.documents.get_mut(&doc.uuid) {
                doc_info.pending_deletion = true;
                println!("  {} Marked for deletion: {}", style("⏳").yellow(), doc.title);
            } else {
                // Document not in state.json yet (config-only), no need to mark
                println!("  {} Will remove from config: {}", style("○").dim(), doc.title);
            }
        }
        println!();
    }

    // Delete local files
    println!("{}", style("Deleting local files...").dim());

    for file_path in &files_to_delete {
        match fs::remove_file(file_path) {
            Ok(_) => {
                println!("  {} Deleted file: {}",
                    style("✓").green(),
                    file_path.display()
                );
            }
            Err(e) => {
                println!("  {} Failed to delete file: {} - {}",
                    style("✗").red(),
                    file_path.display(),
                    e
                );
            }
        }
    }

    // Clean up empty directories
    for target_path in &target_paths {
        if target_path.is_dir() {
            let _ = remove_empty_directories(target_path);
        }
    }

    println!();

    // Update docuram.json - remove deleted documents
    let deleted_uuids: HashSet<String> = docs_to_delete.iter()
        .map(|d| d.uuid.clone())
        .collect();

    let original_documents_count = docuram_config.documents.len();
    docuram_config.documents.retain(|doc| !deleted_uuids.contains(&doc.uuid));
    let removed_from_documents = original_documents_count - docuram_config.documents.len();

    let original_requires_count = docuram_config.requires.len();
    docuram_config.requires.retain(|doc| !deleted_uuids.contains(&doc.uuid));
    let removed_from_requires = original_requires_count - docuram_config.requires.len();

    let removed_from_config = removed_from_documents + removed_from_requires;

    // Update state.json
    // - Remove local-only documents (not synced) completely from state.json
    // - Keep uploaded documents in state.json with pending_deletion flag for push to handle
    for doc in &local_only_docs {
        local_state.documents.remove(&doc.uuid);
    }
    // Config-only documents are not in state.json, so no need to update

    // Save updated configs
    docuram_config.save()
        .context("Failed to save docuram.json")?;
    local_state.save()
        .context("Failed to save state.json")?;

    println!("{}", style("Summary:").bold());
    println!("  {} file(s) deleted locally", files_to_delete.len());
    println!("  {} document(s) removed from docuram.json", removed_from_config);

    let marked_for_deletion = uploaded_docs.len();
    if marked_for_deletion > 0 {
        println!("  {} document(s) marked for deletion from server", marked_for_deletion);
    }

    println!();
    println!("{}", style("✓ Delete completed locally").green().bold());

    if marked_for_deletion > 0 {
        println!();
        println!("{}", style("Note: Run 'teamturbo push' to delete marked documents from the server.").cyan());
    }

    Ok(())
}

#[derive(Clone)]
struct DocumentToDelete {
    uuid: String,
    title: String,
    path: String,
    category_uuid: String,
    category_path: String,
}

/// Find a document by its file path
fn find_document_by_path(
    docuram_config: &DocuramConfig,
    local_state: &LocalState,
    file_path: &Path,
) -> Option<DocumentToDelete> {
    // Try to match by path in docuram.json (documents and requires)
    for doc in docuram_config.all_documents() {
        let doc_path = PathBuf::from(&doc.path);
        if doc_path == file_path || doc_path.canonicalize().ok() == file_path.canonicalize().ok() {
            return Some(DocumentToDelete {
                uuid: doc.uuid.clone(),
                title: doc.title.clone(),
                path: doc.path.clone(),
                category_uuid: doc.category_uuid.clone(),
                category_path: doc.category_path.clone(),
            });
        }
    }

    // Try to match by path in state.json
    for (uuid, doc_info) in &local_state.documents {
        let doc_path = PathBuf::from(&doc_info.path);
        if doc_path == file_path || doc_path.canonicalize().ok() == file_path.canonicalize().ok() {
            // Find document info from docuram.json
            let doc_from_config = docuram_config.all_documents()
                .find(|d| d.uuid == *uuid);

            let title = doc_from_config
                .map(|d| d.title.clone())
                .unwrap_or_else(|| {
                    file_path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Unknown")
                        .to_string()
                });

            let (category_uuid, category_path) = doc_from_config
                .map(|d| (d.category_uuid.clone(), d.category_path.clone()))
                .unwrap_or_else(|| (String::new(), String::new()));

            return Some(DocumentToDelete {
                uuid: uuid.clone(),
                title,
                path: doc_info.path.clone(),
                category_uuid,
                category_path,
            });
        }
    }

    // Try to read frontmatter from the file itself
    // This handles new documents that haven't been pushed yet
    if file_path.exists() && file_path.extension().and_then(|s| s.to_str()) == Some("md") {
        if let Ok(content) = read_file(file_path) {
            if let Ok(Some((front_matter, _))) = extract_front_matter(&content) {
                if let Some(uuid) = front_matter.uuid {
                    // Get relative path from current directory
                    let relative_path = std::env::current_dir()
                        .ok()
                        .and_then(|cwd| file_path.strip_prefix(&cwd).ok())
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| file_path.to_string_lossy().to_string());

                    return Some(DocumentToDelete {
                        uuid,
                        title: front_matter.title,
                        path: relative_path,
                        category_uuid: String::new(), // New documents don't have category UUID yet
                        category_path: front_matter.category,
                    });
                }
            }
        }
    }

    None
}

/// Find all documents within a directory
fn find_documents_in_directory(
    docuram_config: &DocuramConfig,
    local_state: &LocalState,
    dir_path: &Path,
) -> (Vec<DocumentToDelete>, Vec<PathBuf>) {
    let mut docs = Vec::new();
    let mut files = Vec::new();
    let mut seen_uuids = HashSet::new();
    let mut seen_paths = HashSet::new(); // Track file paths to avoid duplicates

    // Search in docuram.json (documents and requires)
    for doc in docuram_config.all_documents() {
        let doc_path = PathBuf::from(&doc.path);

        // Try canonical path if file exists, otherwise use the path directly
        let matches_dir = if let (Ok(canonical_doc), Ok(canonical_dir)) = (doc_path.canonicalize(), dir_path.canonicalize()) {
            canonical_doc.starts_with(&canonical_dir)
        } else {
            // File doesn't exist, compare paths directly
            // Convert both to absolute paths for comparison
            let abs_doc = if doc_path.is_absolute() {
                doc_path.clone()
            } else {
                std::env::current_dir().ok().map(|cwd| cwd.join(&doc_path)).unwrap_or(doc_path.clone())
            };

            let abs_dir = if dir_path.is_absolute() {
                dir_path.to_path_buf()
            } else {
                std::env::current_dir().ok().map(|cwd| cwd.join(dir_path)).unwrap_or_else(|| dir_path.to_path_buf())
            };

            abs_doc.starts_with(&abs_dir)
        };

        if matches_dir {
            if seen_uuids.insert(doc.uuid.clone()) {
                docs.push(DocumentToDelete {
                    uuid: doc.uuid.clone(),
                    title: doc.title.clone(),
                    path: doc.path.clone(),
                    category_uuid: doc.category_uuid.clone(),
                    category_path: doc.category_path.clone(),
                });

                // Only add to files list if file actually exists
                if doc_path.exists() {
                    if let Ok(canonical_doc) = doc_path.canonicalize() {
                        if seen_paths.insert(canonical_doc) {
                            files.push(doc_path);
                        }
                    }
                }
            }
        }
    }

    // Search in state.json for documents not in docuram.json
    for (uuid, doc_info) in &local_state.documents {
        if seen_uuids.contains(uuid) {
            continue;
        }

        let doc_path = PathBuf::from(&doc_info.path);
        if let Ok(canonical_doc) = doc_path.canonicalize() {
            if let Ok(canonical_dir) = dir_path.canonicalize() {
                if canonical_doc.starts_with(&canonical_dir) {
                    let title = doc_path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Unknown")
                        .to_string();

                    // Try to get category info from docuram.json
                    let (category_uuid, category_path) = docuram_config.all_documents()
                        .find(|d| d.uuid == *uuid)
                        .map(|d| (d.category_uuid.clone(), d.category_path.clone()))
                        .unwrap_or_else(|| (String::new(), String::new()));

                    docs.push(DocumentToDelete {
                        uuid: uuid.clone(),
                        title,
                        path: doc_info.path.clone(),
                        category_uuid,
                        category_path,
                    });
                    if seen_paths.insert(canonical_doc.clone()) {
                        files.push(doc_path);
                    }
                    seen_uuids.insert(uuid.clone());
                }
            }
        }
    }

    // Search for markdown files with frontmatter in the directory
    // This handles new documents that haven't been pushed yet
    if let Ok(canonical_dir) = dir_path.canonicalize() {
        if let Ok(entries) = fs::read_dir(&canonical_dir) {
            for entry in entries.flatten() {
                let file_path = entry.path();

                // Recursively search subdirectories
                if file_path.is_dir() {
                    let (sub_docs, sub_files) = find_documents_in_directory(
                        docuram_config,
                        local_state,
                        &file_path,
                    );
                    for doc in sub_docs {
                        if seen_uuids.insert(doc.uuid.clone()) {
                            docs.push(doc);
                        }
                    }
                    // Only add files that haven't been seen before
                    for sub_file in sub_files {
                        if let Ok(canonical_file) = sub_file.canonicalize() {
                            if seen_paths.insert(canonical_file) {
                                files.push(sub_file);
                            }
                        }
                    }
                } else if file_path.extension().and_then(|s| s.to_str()) == Some("md") {
                    // Try to read frontmatter from markdown files
                    if let Ok(content) = read_file(&file_path) {
                        if let Ok(Some((front_matter, _))) = extract_front_matter(&content) {
                            if let Some(uuid) = front_matter.uuid {
                                if seen_uuids.insert(uuid.clone()) {
                                    // Get relative path from current directory
                                    let relative_path = std::env::current_dir()
                                        .ok()
                                        .and_then(|cwd| file_path.strip_prefix(&cwd).ok())
                                        .map(|p| p.to_string_lossy().to_string())
                                        .unwrap_or_else(|| file_path.to_string_lossy().to_string());

                                    docs.push(DocumentToDelete {
                                        uuid,
                                        title: front_matter.title,
                                        path: relative_path,
                                        category_uuid: String::new(), // New documents don't have category UUID yet
                                        category_path: front_matter.category,
                                    });
                                    if let Ok(canonical_file) = file_path.canonicalize() {
                                        if seen_paths.insert(canonical_file) {
                                            files.push(file_path);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    (docs, files)
}

/// Remove empty directories recursively
fn remove_empty_directories(dir_path: &Path) -> Result<()> {
    if !dir_path.is_dir() {
        return Ok(());
    }

    // First, try to remove empty subdirectories
    let entries = fs::read_dir(dir_path)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let _ = remove_empty_directories(&path);
        }
    }

    // Then try to remove this directory if it's empty
    // Don't remove the docuram/ directory itself
    if dir_path.file_name() != Some(std::ffi::OsStr::new("docuram")) {
        if let Ok(mut entries) = fs::read_dir(dir_path) {
            if entries.next().is_none() {
                let _ = fs::remove_dir(dir_path);
            }
        }
    }

    Ok(())
}
