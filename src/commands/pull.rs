use anyhow::{Context, Result};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashSet;
use std::path::PathBuf;
use std::fs;

use crate::api::ApiClient;
use crate::config::{CliConfig, DocuramConfig, CategoryTree};
use crate::utils::{storage::LocalState, write_file, read_file, calculate_checksum, logger};

pub async fn execute(documents: Vec<String>, force: bool) -> Result<()> {
    println!("{}", style("Pull Document Updates").cyan().bold());
    println!();

    // Load docuram config
    let mut docuram_config = DocuramConfig::load()
        .context("Failed to load docuram/docuram.json. Run 'teamturbo init' first.")?;

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

    // Fetch updated config to get the latest category_tree
    println!("{}", style("Fetching updated category tree...").dim());
    let config_url = format!("{}/docuram/categories/{}/generate_config",
        server_url, category_uuid);
    let updated_config = client.get_docuram_config(&config_url).await?;

    // Update category_tree in local config if it exists
    if let Some(ref category_tree) = updated_config.category_tree {
        // Convert api::client::CategoryTree to config::CategoryTree
        let config_tree = convert_category_tree(category_tree);
        docuram_config.category_tree = Some(config_tree.clone());

        // Create empty category directories from updated tree
        println!("{}", style("Creating category directories...").dim());
        let created_count = create_category_directories(&config_tree, "docuram")?;
        if created_count > 0 {
            println!("{}", style(format!("✓ Created {} new category director(ies)", created_count)).green());
        }

        // Save updated config with new category_tree
        docuram_config.save()
            .context("Failed to save updated docuram.json")?;
    }
    println!();

    // Check for new documents (not in docuram.json)
    let local_doc_uuids: HashSet<String> = docuram_config
        .all_documents()
        .map(|doc| doc.uuid.clone())
        .collect();

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
                is_required: false,
            };

            // Add document to the documents array
            docuram_config.documents.push(new_doc_info);
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
        let file_path = PathBuf::from(&doc_info.path);

        // Check local state
        let local_info = local_state.get_document(&doc_info.uuid);

        if file_path.exists() {
            // File exists, check if it has been modified locally
            let current_content = read_file(&file_path)?;

            // Calculate checksum of complete content (including frontmatter)
            let current_checksum = calculate_checksum(&current_content);

            let is_modified = match local_info {
                Some(info) => current_checksum != info.checksum,
                None => true, // No local state, assume modified
            };

            if is_modified && !force {
                // Local modifications detected
                conflicts.push(doc_info.uuid.clone());
            } else {
                // Check if remote has updates by comparing versions
                let local_version = local_info.map(|info| info.version).unwrap_or(0);
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

    for doc_info in to_update {
        pb.set_message(format!("{}", doc_info.title));

        match pull_document(&client, doc_info, &mut local_state).await {
            Ok(_) => {
                success_count += 1;
            }
            Err(e) => {
                failed_docs.push((doc_info.uuid.clone(), e.to_string()));
            }
        }

        pb.inc(1);
    }

    pb.finish_with_message("Done");

    // Save local state
    local_state.save()
        .context("Failed to save local state")?;

    println!();
    if failed_docs.is_empty() {
        println!("{}", style(format!("✓ Successfully pulled {} documents", success_count)).green());
    } else {
        println!("{}", style(format!("✓ Pulled {} documents", success_count)).green());
        println!("{}", style(format!("✗ Failed to pull {} documents:", failed_docs.len())).red());
        for (slug, error) in failed_docs {
            println!("  - {}: {}", slug, error);
        }
    }

    Ok(())
}

/// Pull a single document
async fn pull_document(
    client: &ApiClient,
    doc_info: &crate::config::DocumentInfo,
    local_state: &mut LocalState,
) -> Result<()> {
    // Download document content
    let doc = client.download_document(&doc_info.uuid).await?;

    // Backend now stores complete content with frontmatter, so no need to add it
    let full_content = doc.content.unwrap_or_default();

    // Write to file
    let file_path = PathBuf::from(&doc_info.path);
    write_file(&file_path, &full_content)
        .with_context(|| format!("Failed to write document to {:?}", file_path))?;

    // Calculate checksum of complete content (including frontmatter)
    let content_checksum = crate::utils::calculate_checksum(&full_content);

    // Update local state
    local_state.upsert_document(crate::utils::storage::LocalDocumentInfo {
        uuid: doc_info.uuid.clone(),
        path: doc_info.path.clone(),
        checksum: content_checksum,
        version: doc.version,
        last_sync: chrono::Utc::now().to_rfc3339(),
        pending_deletion: false,
    });

    Ok(())
}

/// Add docuram metadata to document content
fn add_docuram_metadata(content: &str, doc_info: &crate::config::DocumentInfo, version: i64) -> Result<String> {
    use crate::utils::logger;

    // Check if metadata already exists
    if content.starts_with("---\ndocuram:") || content.starts_with("---\r\ndocuram:") {
        logger::debug("metadata", "Document already has docuram metadata, skipping");
        return Ok(content.to_string());
    }

    // Build metadata frontmatter (without synced_at to avoid checksum changes)
    let metadata = format!(
        r#"---
docuram:
  schema: "TEAMTURBO DOCURAM DOCUMENT"
  uuid: "{}"
  title: "{}"
  category: "{}"
  category_uuid: "{}"
  doc_type: "{}"
  version: {}
---

"#,
        doc_info.uuid,
        doc_info.title.replace('"', "\\\""),
        doc_info.category_path.replace('"', "\\\""),
        doc_info.category_uuid,
        doc_info.doc_type,
        version
    );

    // Prepend metadata to content
    Ok(format!("{}{}", metadata, content))
}

/// Convert api::client::CategoryTree to config::CategoryTree
fn convert_category_tree(api_tree: &crate::api::client::CategoryTree) -> CategoryTree {
    CategoryTree {
        id: api_tree.id,
        name: api_tree.name.clone(),
        slug: api_tree.slug.clone(),
        path: api_tree.path.clone(),
        description: api_tree.description.clone(),
        position: api_tree.position,
        parent_id: api_tree.parent_id,
        subcategories: api_tree.subcategories.as_ref().map(|subs| {
            subs.iter().map(|sub| convert_category_tree(sub)).collect()
        }),
        document_count: api_tree.document_count,
        created_at: api_tree.created_at.clone(),
        updated_at: api_tree.updated_at.clone(),
    }
}

/// Recursively create empty category directories
/// Returns the count of directories created
fn create_category_directories(category: &CategoryTree, root_path: &str) -> Result<usize> {
    let mut count = 0;

    // Use the category's full path and prepend root_path (e.g., "docuram")
    let full_path = if root_path.is_empty() {
        category.path.clone()
    } else {
        format!("{}/{}", root_path, category.path)
    };

    // Create directory if it doesn't exist and has no documents
    let dir_path = PathBuf::from(&full_path);
    if category.document_count == 0 && !dir_path.exists() {
        fs::create_dir_all(&dir_path)
            .with_context(|| format!("Failed to create directory: {:?}", dir_path))?;
        logger::debug("create_dir", &format!("Created empty category directory: {:?}", dir_path));
        count += 1;
    }

    // Recursively create subdirectories
    if let Some(ref subcategories) = category.subcategories {
        for subcat in subcategories {
            count += create_category_directories(subcat, root_path)?;
        }
    }

    Ok(count)
}
