use anyhow::{Context, Result};
use console::style;
use dialoguer::Confirm;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::DocuramConfig;

pub async fn execute(paths: Vec<String>, force: bool, _verbose: bool) -> Result<()> {
    println!();
    println!("{}", style("Delete Documents").bold());
    println!();

    if paths.is_empty() {
        anyhow::bail!("No paths specified. Please provide at least one document or directory path.");
    }

    // Load docuram config with migration from state.json
    let mut docuram_config = DocuramConfig::load_with_migration()
        .context("Failed to load docuram.json. Make sure you're in a docuram project directory.")?;

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

    // Get working category path
    let working_category_path = &docuram_config.docuram.category_path;

    // Collect all documents to delete (including documents in directories)
    let mut docs_to_delete = Vec::new();
    let mut files_to_delete = Vec::new();

    for target_path in &target_paths {
        if target_path.is_file() {
            // Single file - find matching document
            if let Some(doc) = find_document_by_path(&docuram_config, target_path, working_category_path) {
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
                target_path,
                working_category_path
            );

            if dir_docs.is_empty() {
                // If still no documents found, try as a single file
                if let Some(doc) = find_document_by_path(&docuram_config, target_path, working_category_path) {
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

        // Check if document has been synced (has local_checksum in docuram.json)
        let doc_info = docuram_config.get_document_by_uuid(&doc.uuid);
        let is_synced = doc_info.map(|d| d.local_checksum.is_some()).unwrap_or(false);

        if is_synced {
            // Document has been synced, meaning it was uploaded
            uploaded_docs.push(doc.clone());
        } else if file_exists {
            // Document exists locally but not synced (local-only)
            local_only_docs.push(doc.clone());
        } else {
            // Document is in docuram.json but file doesn't exist and not synced (config-only)
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

    // Mark uploaded documents as pending deletion in docuram.json
    let server_docs: Vec<_> = uploaded_docs.iter().collect();
    if !server_docs.is_empty() {
        println!("{}", style("Marking documents for deletion from server...").dim());

        for doc in &server_docs {
            // Mark for deletion in docuram.json
            if docuram_config.mark_for_deletion(&doc.uuid) {
                println!("  {} Marked for deletion: {}", style("⏳").yellow(), doc.title);
            } else {
                println!("  {} Document not found in config: {}", style("○").dim(), doc.title);
            }
        }
        println!();
    }

    // Config-only documents will just be removed from docuram.json
    if !config_only_docs.is_empty() {
        println!("{}", style("Will remove from config:").dim());
        for doc in &config_only_docs {
            println!("  {} {}", style("○").dim(), doc.title);
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

    // Local-only and config-only documents are removed from docuram.json entirely
    // (they were not synced, so no need to mark for deletion)
    for doc in &local_only_docs {
        docuram_config.remove_document_by_uuid(&doc.uuid);
    }
    for doc in &config_only_docs {
        docuram_config.remove_document_by_uuid(&doc.uuid);
    }

    // Save updated config
    docuram_config.save()
        .context("Failed to save docuram.json")?;

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
    file_path: &Path,
    working_category_path: &str,
) -> Option<DocumentToDelete> {
    // Try to match by path in docuram.json (documents and requires)
    for doc in docuram_config.all_documents() {
        // Use local_path() to get correct path (dependencies go in dependencies/ at project root)
        let local_file_path = doc.local_path(working_category_path);
        let doc_path = PathBuf::from(&local_file_path);
        if doc_path == file_path || doc_path.canonicalize().ok() == file_path.canonicalize().ok() {
            return Some(DocumentToDelete {
                uuid: doc.uuid.clone(),
                title: doc.title.clone(),
                path: local_file_path,  // Store the local path
                category_uuid: doc.category_uuid.clone(),
                category_path: doc.category_path.clone(),
            });
        }
    }

    // Handle markdown files that exist locally but aren't in docuram.json
    // These are local-only files that can still be deleted
    if file_path.exists() && file_path.extension().and_then(|s| s.to_str()) == Some("md") {
        // Get relative path from current directory
        let relative_path = std::env::current_dir()
            .ok()
            .and_then(|cwd| file_path.strip_prefix(&cwd).ok())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| file_path.to_string_lossy().to_string());

        let title = file_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string();

        return Some(DocumentToDelete {
            uuid: format!("local:{}", relative_path),
            title,
            path: relative_path,
            category_uuid: String::new(),
            category_path: String::new(),
        });
    }

    None
}

/// Find all documents within a directory
fn find_documents_in_directory(
    docuram_config: &DocuramConfig,
    dir_path: &Path,
    working_category_path: &str,
) -> (Vec<DocumentToDelete>, Vec<PathBuf>) {
    let mut docs = Vec::new();
    let mut files = Vec::new();
    let mut seen_uuids = HashSet::new();
    let mut seen_paths = HashSet::new(); // Track file paths to avoid duplicates

    // Search in docuram.json (documents and requires)
    for doc in docuram_config.all_documents() {
        // Use local_path() to get correct path (dependencies go in dependencies/ at project root)
        let local_file_path = doc.local_path(working_category_path);
        let doc_path = PathBuf::from(&local_file_path);

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
                    path: local_file_path.clone(),  // Use local path
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

    // Search for markdown files in the directory
    // This handles new documents that haven't been pushed yet
    if let Ok(canonical_dir) = dir_path.canonicalize() {
        if let Ok(entries) = fs::read_dir(&canonical_dir) {
            for entry in entries.flatten() {
                let file_path = entry.path();

                // Recursively search subdirectories
                if file_path.is_dir() {
                    let (sub_docs, sub_files) = find_documents_in_directory(
                        docuram_config,
                        &file_path,
                        working_category_path,
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
                    // Handle markdown files that exist locally but aren't in docuram.json
                    let relative_path = std::env::current_dir()
                        .ok()
                        .and_then(|cwd| file_path.strip_prefix(&cwd).ok())
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| file_path.to_string_lossy().to_string());

                    let uuid = format!("local:{}", relative_path);
                    if seen_uuids.insert(uuid.clone()) {
                        let title = file_path.file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("Unknown")
                            .to_string();

                        docs.push(DocumentToDelete {
                            uuid,
                            title,
                            path: relative_path,
                            category_uuid: String::new(),
                            category_path: String::new(),
                        });
                    }

                    if let Ok(canonical_file) = file_path.canonicalize() {
                        if seen_paths.insert(canonical_file) {
                            files.push(file_path);
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
    // Don't remove the docuram/ or dependencies/ directories themselves
    let dir_name = dir_path.file_name();
    if dir_name != Some(std::ffi::OsStr::new("docuram")) &&
       dir_name != Some(std::ffi::OsStr::new("dependencies")) {
        if let Ok(mut entries) = fs::read_dir(dir_path) {
            if entries.next().is_none() {
                let _ = fs::remove_dir(dir_path);
            }
        }
    }

    Ok(())
}
