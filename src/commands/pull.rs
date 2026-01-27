use anyhow::{Context, Result};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::{HashSet, HashMap};
use std::path::PathBuf;
use std::fs;

use crate::api::{ApiClient, PublicApiClient};
use crate::config::{CliConfig, DocuramConfig, DocumentInfo, PublicDependency};
use crate::utils::{write_file, read_file, calculate_checksum, logger};

pub async fn execute(documents: Vec<String>, force: bool) -> Result<()> {
    println!("{}", style("Pull Document Updates").cyan().bold());
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

    // Get category UUID from docuram config
    let category_uuid = match &docuram_config.docuram.category_uuid {
        Some(uuid) => uuid.clone(),
        None => anyhow::bail!("No category UUID in docuram.json"),
    };

    // Fetch all remote documents (including dependencies and new documents)
    println!("{}", style("Fetching remote documents...").dim());
    let remote_docs = client.get_document_versions(&category_uuid).await?;

    // Build a map of remote versions for quick lookup
    let remote_versions: std::collections::HashMap<String, i64> = remote_docs
        .iter()
        .map(|doc| (doc.uuid.clone(), doc.version))
        .collect();

    // Ensure document type directories exist
    println!("{}", style("Ensuring document type directories exist...").dim());
    let mut created_count = 0;

    let organic_path = PathBuf::from("docuram/organic");
    if !organic_path.exists() {
        fs::create_dir_all(&organic_path)
            .context("Failed to create organic directory")?;
        logger::debug("create_dir", &format!("Created directory: {:?}", organic_path));
        created_count += 1;
    }

    let impl_path = PathBuf::from("docuram/impl");
    if !impl_path.exists() {
        fs::create_dir_all(&impl_path)
            .context("Failed to create impl directory")?;
        logger::debug("create_dir", &format!("Created directory: {:?}", impl_path));
        created_count += 1;
    }

    let dependencies_path = PathBuf::from("dependencies");
    if !dependencies_path.exists() {
        fs::create_dir_all(&dependencies_path)
            .context("Failed to create dependencies directory")?;
        logger::debug("create_dir", &format!("Created dependencies directory: {:?}", dependencies_path));
        created_count += 1;
    }

    if created_count > 0 {
        println!("{}", style(format!("✓ Created {} director(ies)", created_count)).green());
    }
    println!();

    // Check for new documents (not in docuram.json)
    let local_doc_uuids: HashSet<String> = docuram_config
        .all_documents()
        .map(|doc| doc.uuid.clone())
        .collect();

    // Check for remote document UUIDs
    let remote_doc_uuids: HashSet<String> = remote_docs
        .iter()
        .map(|doc| doc.uuid.clone())
        .collect();

    // Check for documents deleted on server (in local but not in remote)
    let deleted_on_server: Vec<_> = docuram_config
        .all_documents()
        .filter(|doc| !remote_doc_uuids.contains(&doc.uuid))
        .map(|doc| (doc.uuid.clone(), doc.title.clone(), doc.local_path(&docuram_config.docuram.category_path)))
        .collect();

    if !deleted_on_server.is_empty() {
        println!("{}", style(format!("🗑 {} document(s) deleted on server, removing locally:", deleted_on_server.len())).yellow());
        for (uuid, title, local_path) in &deleted_on_server {
            println!("  - {} ({})", title, uuid);
            // Delete local file if exists
            let file_path = PathBuf::from(local_path);
            if file_path.exists() {
                let _ = fs::remove_file(&file_path);
            }
            // Remove from docuram.json
            docuram_config.remove_document_by_uuid(uuid);
        }
        // Save updated docuram config
        docuram_config.save()
            .context("Failed to save docuram.json after removing deleted documents")?;
        println!();
    }

    let new_docs: Vec<_> = remote_docs
        .iter()
        .filter(|doc| !local_doc_uuids.contains(&doc.uuid))
        .collect();

    if !new_docs.is_empty() {
        println!();
        println!("{}", style(format!("Found {} new document(s) from dependencies:", new_docs.len())).yellow());
        for doc in &new_docs {
            println!("  + {}/{}", doc.category_path, doc.title);
        }
        println!();

        // Add new documents to docuram config
        for doc in &new_docs {
            let new_doc_info = crate::config::DocumentInfo {
                id: doc.id,
                uuid: doc.uuid.clone(),
                title: doc.title.clone(),
                category_id: doc.category_id,
                category_name: doc.category_name.clone(),
                category_path: doc.category_path.clone(),
                category_uuid: doc.category_uuid.clone(),
                doc_type: doc.doc_type.clone(),
                version: doc.version,
                path: doc.path.clone(),
                checksum: doc.checksum.clone(),
                is_required: doc.is_required,
                // Local state fields - initially empty, will be set after download
                local_checksum: None,
                last_sync: None,
                pending_deletion: false,
            };

            // Add document to appropriate array based on is_required flag
            if new_doc_info.is_required {
                docuram_config.requires.push(new_doc_info);
            } else {
                docuram_config.documents.push(new_doc_info);
            }
        }

        // Save updated docuram config
        docuram_config.save()
            .context("Failed to save updated docuram.json")?;
        println!("{}", style("Updated docuram.json with new documents").green());
        println!();
    }

    // Determine which documents to pull
    let docs_to_pull: Vec<_> = if documents.is_empty() {
        // Pull all documents (including newly added ones)
        docuram_config.all_documents().collect()
    } else {
        // Pull specific documents
        let doc_set: HashSet<String> = documents.into_iter().collect();
        docuram_config
            .all_documents()
            .filter(|doc| doc_set.contains(&doc.uuid))
            .collect()
    };

    if docs_to_pull.is_empty() {
        println!("{}", style("No documents to pull").yellow());
        return Ok(());
    }

    println!("Checking {} document(s)...", docs_to_pull.len());
    println!();

    // Check which documents need updating
    let mut to_update = Vec::new();
    let mut to_skip = Vec::new();
    let mut conflicts = Vec::new();

    for doc_info in &docs_to_pull {
        // Use local_path() to get correct path (dependencies go in working_category/dependencies/ subdirectory)
        let working_category_path = &docuram_config.docuram.category_path;
        let local_file_path = doc_info.local_path(working_category_path);
        let file_path = PathBuf::from(&local_file_path);

        if file_path.exists() {
            // File exists, check if it has been modified locally
            let current_content = read_file(&file_path)?;

            // Calculate checksum of complete content
            let current_checksum = calculate_checksum(&current_content);

            // Check if local file has been modified since last sync
            let is_modified = match &doc_info.local_checksum {
                Some(local_cs) => current_checksum != *local_cs,
                None => true, // No local checksum, assume modified
            };

            if is_modified && !force {
                // Local modifications detected
                conflicts.push(doc_info.uuid.clone());
            } else {
                // Check if remote has updates by comparing versions
                let local_version = if doc_info.local_checksum.is_some() { doc_info.version } else { 0 };
                let remote_version = remote_versions.get(&doc_info.uuid).copied().unwrap_or(doc_info.version);

                if remote_version > local_version {
                    // Remote has newer version, needs update
                    to_update.push(doc_info);
                } else {
                    // Local is up to date
                    to_skip.push(doc_info.uuid.clone());
                }
            }
        } else {
            // File doesn't exist, needs download
            to_update.push(doc_info);
        }
    }

    // Report conflicts
    if !conflicts.is_empty() {
        println!("{}", style(format!("⚠ {} document(s) have local modifications:", conflicts.len())).yellow());
        for slug in &conflicts {
            println!("  - {}", slug);
        }
        println!();
        println!("{}", style("Use --force to overwrite local changes").dim());
        println!();
    }

    // Report skip
    if !to_skip.is_empty() {
        println!("{}", style(format!("✓ {} document(s) already up to date", to_skip.len())).green());
    }

    // Pull updates
    if to_update.is_empty() {
        println!();
        println!("{}", style("All documents are up to date").green());

        // Still check public dependencies even when local docs are up to date
        println!();
        pull_public_dependencies(&mut docuram_config, force).await?;

        return Ok(());
    }

    println!();
    println!("{}", style(format!("Pulling {} document(s)...", to_update.len())).bold());
    println!();

    // Create progress bar
    let pb = ProgressBar::new(to_update.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .expect("Invalid progress bar template")
            .progress_chars("=> ")
    );

    let mut success_count = 0;
    let mut failed_docs = Vec::new();
    let mut deleted_docs = Vec::new();

    // Collect UUIDs to update
    let uuids_to_update: Vec<String> = to_update.iter().map(|d| d.uuid.clone()).collect();

    for doc_uuid in &uuids_to_update {
        // Get doc_info for progress message
        let title = docuram_config.get_document_by_uuid(doc_uuid)
            .map(|d| d.title.clone())
            .unwrap_or_default();
        pb.set_message(format!("{}", title));

        let working_category_path = docuram_config.docuram.category_path.clone();
        match pull_document(&client, doc_uuid, &mut docuram_config, &working_category_path).await {
            Ok(_) => {
                success_count += 1;
            }
            Err(e) => {
                let error_msg = e.to_string();
                // Check if the error indicates document was deleted on server
                if error_msg.contains("not found") || error_msg.contains("Not found") || error_msg.contains("404") {
                    // Document was deleted on server, remove from local
                    let doc_info = docuram_config.get_document_by_uuid(doc_uuid);
                    if let Some(info) = doc_info {
                        let local_path = info.local_path(&working_category_path);
                        // Delete local file if exists
                        let file_path = PathBuf::from(&local_path);
                        if file_path.exists() {
                            let _ = fs::remove_file(&file_path);
                        }
                    }
                    // Remove from docuram.json
                    docuram_config.remove_document_by_uuid(doc_uuid);
                    deleted_docs.push((doc_uuid.clone(), title.clone()));
                } else {
                    failed_docs.push((doc_uuid.clone(), error_msg));
                }
            }
        }

        pb.inc(1);
    }

    pb.finish_with_message("Done");

    // Save updated docuram config
    docuram_config.save()
        .context("Failed to save docuram.json")?;

    println!();
    if !deleted_docs.is_empty() {
        println!("{}", style(format!("🗑 Removed {} document(s) deleted from server:", deleted_docs.len())).yellow());
        for (uuid, title) in &deleted_docs {
            println!("  - {} ({})", title, uuid);
        }
    }
    if failed_docs.is_empty() && deleted_docs.is_empty() {
        println!("{}", style(format!("✓ Successfully pulled {} documents", success_count)).green());
    } else if failed_docs.is_empty() {
        println!("{}", style(format!("✓ Pulled {} documents", success_count)).green());
    } else {
        println!("{}", style(format!("✓ Pulled {} documents", success_count)).green());
        println!("{}", style(format!("✗ Failed to pull {} documents:", failed_docs.len())).red());
        for (slug, error) in failed_docs {
            println!("  - {}: {}", slug, error);
        }
    }

    // Pull public dependencies updates
    println!();
    pull_public_dependencies(&mut docuram_config, force).await?;

    Ok(())
}

/// Pull a single document
async fn pull_document(
    client: &ApiClient,
    doc_uuid: &str,
    docuram_config: &mut DocuramConfig,
    working_category_path: &str,
) -> Result<()> {
    // Download document content
    let doc = client.download_document(doc_uuid).await?;

    // Get pure content without frontmatter
    let content = doc.content.unwrap_or_default();

    // Get document info to calculate local path
    let doc_info = docuram_config.get_document_by_uuid(doc_uuid)
        .context("Document not found in config")?;
    let local_file_path = doc_info.local_path(working_category_path);
    let file_path = PathBuf::from(&local_file_path);

    write_file(&file_path, &content)
        .with_context(|| format!("Failed to write document to {:?}", file_path))?;

    // Calculate checksum of content
    let content_checksum = crate::utils::calculate_checksum(&content);

    // Update document's local state in docuram config
    if let Some(doc_mut) = docuram_config.get_document_by_uuid_mut(doc_uuid) {
        doc_mut.local_checksum = Some(content_checksum);
        doc_mut.last_sync = Some(chrono::Utc::now().to_rfc3339());
        doc_mut.version = doc.version;
        doc_mut.pending_deletion = false;
    }

    Ok(())
}

/// Pull public dependencies updates from docuram.teamturbo.io
async fn pull_public_dependencies(docuram_config: &mut DocuramConfig, force: bool) -> Result<()> {
    println!("{}", style("Checking public dependencies from Docuram Official...").bold());

    let public_client = PublicApiClient::new(PublicApiClient::default_url().to_string());

    // Fetch global dependencies list
    let global_deps = match public_client.get_global_dependencies().await {
        Ok(deps) => deps,
        Err(e) => {
            println!("{}", style(format!("⚠ Could not fetch public dependencies: {}", e)).yellow());
            return Ok(());
        }
    };

    if global_deps.global_dependencies.is_empty() {
        println!("{}", style("  No public dependencies available").dim());
        return Ok(());
    }

    // Build map of existing public dependency documents by UUID
    let mut existing_docs: HashMap<String, (usize, usize, i64)> = HashMap::new(); // uuid -> (dep_idx, doc_idx, version)
    for (dep_idx, dep) in docuram_config.public_dependencies.iter().enumerate() {
        for (doc_idx, doc) in dep.documents.iter().enumerate() {
            existing_docs.insert(doc.uuid.clone(), (dep_idx, doc_idx, doc.version));
        }
    }

    // Build set of existing public dependency category UUIDs
    let existing_categories: HashSet<String> = docuram_config
        .public_dependencies
        .iter()
        .map(|d| d.category_uuid.clone())
        .collect();

    let deps_dir = PathBuf::from("dependencies");
    if !deps_dir.exists() {
        fs::create_dir_all(&deps_dir)
            .context("Failed to create dependencies directory")?;
    }

    let mut new_docs_count = 0;
    let mut updated_docs_count = 0;
    let mut new_categories_count = 0;

    for dep_category in &global_deps.global_dependencies {
        // Download the dependency's documents
        let download_result = match public_client.download_global_dependency(&dep_category.uuid).await {
            Ok(result) => result,
            Err(e) => {
                println!("{}", style(format!("  ⚠ Failed to fetch {}: {}", dep_category.name, e)).yellow());
                continue;
            }
        };

        // Check if this is a new category
        let is_new_category = !existing_categories.contains(&dep_category.uuid);
        if is_new_category {
            new_categories_count += 1;
        }

        let mut category_docs: Vec<DocumentInfo> = Vec::new();
        let mut category_updated = false;

        for doc in &download_result.documents {
            let relative_path = doc.path.strip_prefix("docuram/").unwrap_or(&doc.path);
            let local_path = deps_dir.join(relative_path);

            // Check if document exists and needs update
            let (needs_download, is_new) = if let Some((_, _, local_version)) = existing_docs.get(&doc.uuid) {
                if doc.version > *local_version {
                    (true, false) // Needs update
                } else if !local_path.exists() {
                    (true, false) // File missing
                } else if force {
                    (true, false) // Force update
                } else {
                    (false, false) // Up to date
                }
            } else {
                (true, true) // New document
            };

            if needs_download {
                let content = doc.content.clone().unwrap_or_default();

                // Ensure parent directory exists
                if let Some(parent) = local_path.parent() {
                    if !parent.exists() {
                        fs::create_dir_all(parent)?;
                    }
                }

                // Write the document
                write_file(&local_path, &content)
                    .with_context(|| format!("Failed to write document: {:?}", local_path))?;

                let checksum = calculate_checksum(&content);

                if is_new {
                    new_docs_count += 1;
                    println!("  {} {} [PUBLIC]", style("+").green(), doc.title);
                } else {
                    updated_docs_count += 1;
                    println!("  {} {} [PUBLIC] (v{} → v{})",
                        style("↑").cyan(), doc.title,
                        existing_docs.get(&doc.uuid).map(|(_, _, v)| *v).unwrap_or(0),
                        doc.version);
                }

                category_docs.push(DocumentInfo {
                    id: doc.id,
                    uuid: doc.uuid.clone(),
                    title: doc.title.clone(),
                    category_id: doc.category_id,
                    category_name: doc.category_name.clone(),
                    category_path: doc.category_path.clone(),
                    category_uuid: doc.category_uuid.clone(),
                    doc_type: doc.doc_type.clone(),
                    version: doc.version,
                    path: format!("dependencies/{}", relative_path),
                    checksum: doc.checksum.clone(),
                    is_required: true,
                    local_checksum: Some(checksum),
                    last_sync: Some(chrono::Utc::now().to_rfc3339()),
                    pending_deletion: false,
                });
                category_updated = true;
            } else {
                // Keep existing document info
                if let Some((dep_idx, doc_idx, _)) = existing_docs.get(&doc.uuid) {
                    if let Some(existing_doc) = docuram_config.public_dependencies
                        .get(*dep_idx)
                        .and_then(|d| d.documents.get(*doc_idx))
                    {
                        category_docs.push(existing_doc.clone());
                    }
                }
            }
        }

        // Update or add the category in public_dependencies
        if category_updated || is_new_category {
            let public_dep = PublicDependency {
                category_uuid: dep_category.uuid.clone(),
                category_name: dep_category.name.clone(),
                category_path: dep_category.path.clone(),
                source_url: global_deps.source.url.clone(),
                document_count: category_docs.len() as i64,
                documents: category_docs,
            };

            // Find and replace existing or add new
            if let Some(idx) = docuram_config.public_dependencies
                .iter()
                .position(|d| d.category_uuid == dep_category.uuid)
            {
                docuram_config.public_dependencies[idx] = public_dep;
            } else {
                docuram_config.public_dependencies.push(public_dep);
            }
        }
    }

    // Save updated config
    if new_docs_count > 0 || updated_docs_count > 0 || new_categories_count > 0 {
        docuram_config.save()
            .context("Failed to save docuram.json")?;

        println!();
        if new_categories_count > 0 {
            println!("{}", style(format!("✓ Added {} new public dependency categor(ies)", new_categories_count)).green());
        }
        if new_docs_count > 0 {
            println!("{}", style(format!("✓ Downloaded {} new public document(s)", new_docs_count)).green());
        }
        if updated_docs_count > 0 {
            println!("{}", style(format!("✓ Updated {} public document(s)", updated_docs_count)).green());
        }
    } else {
        println!("{}", style("✓ All public dependencies are up to date").green());
    }

    Ok(())
}

