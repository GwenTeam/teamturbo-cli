use anyhow::{Context, Result};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::{Path, PathBuf};
use dialoguer::Confirm;

use crate::api::ApiClient;
use crate::api::client::{DocumentInfo, CategoryTree};
use crate::config::CliConfig;
use crate::utils::{storage::LocalState, write_file, logger, calculate_checksum};

pub async fn execute(config_url: Option<String>, force: bool, no_download: bool) -> Result<()> {
    println!("{}", style("Initialize Docuram Project").cyan().bold());
    println!();

    // Check if docuram/docuram.json already exists
    let config_path = Path::new("docuram").join("docuram.json");
    if config_path.exists() && !force {
        anyhow::bail!(
            "docuram/docuram.json already exists. Use --force to overwrite, or run 'teamturbo pull' to update documents."
        );
    }

    // Get config source
    let config_source = match config_url {
        Some(url) => url,
        None => {
            anyhow::bail!(
                "No config URL specified. Use --config-url <url> to specify config URL.\n\
                 Example: teamturbo init --config-url http://127.0.0.1:3001/docuram/categories/1/generate_config"
            );
        }
    };

    // Load CLI config to get auth
    let cli_config = CliConfig::load()?;
    logger::debug("init", "Loaded CLI config");

    // Determine server URL from config URL
    let server_url = extract_server_url(&config_source)?;
    logger::debug("init", &format!("Server URL: {}", server_url));

    // Get auth for this server
    let auth = cli_config
        .get_auth(&server_url)
        .context(format!("Not logged in to {}. Run 'teamturbo login' first.", server_url))?;
    logger::debug("init", "Authentication token found");

    // Create API client
    let client = ApiClient::new(server_url.clone(), auth.access_token.clone());

    // Download docuram config
    println!("Downloading configuration from {}...", style(&config_source).cyan());
    let docuram_config = client.get_docuram_config(&config_source).await?;

    // Ensure docuram directory exists
    fs::create_dir_all("docuram")
        .context("Failed to create docuram directory")?;

    // Save docuram/docuram.json
    println!("Saving {}...", style("docuram/docuram.json").cyan());
    let config_json = serde_json::to_string_pretty(&docuram_config)
        .context("Failed to serialize config")?;
    fs::write(&config_path, config_json)
        .context("Failed to write docuram/docuram.json")?;

    println!("{}", style("✓ Configuration saved").green());
    println!();

    // Display project info
    println!("{}", style("Project Information:").bold());
    println!("  Name: {}", docuram_config.project.name);
    if let Some(desc) = &docuram_config.project.description {
        println!("  Description: {}", desc);
    }
    println!("  Category: {}", docuram_config.docuram.category_path);
    if let Some(task_name) = &docuram_config.docuram.task_name {
        println!("  Task: {}", task_name);
    }
    println!();

    // Count all documents (working documents + dependencies)
    let total_docs = docuram_config.documents.len() + docuram_config.requires.len();

    println!("{}", style(format!("Documents: {} total ({} working, {} dependencies)",
        total_docs,
        docuram_config.documents.len(),
        docuram_config.requires.len())).bold());

    if no_download {
        println!();
        println!("{}", style("⚠ Skipping document download (--no-download flag)").yellow());
        println!();
        println!("{}", style("Project structure created. Run 'teamturbo pull' to download documents.").dim());
        return Ok(());
    }

    // Confirm download
    if !force {
        println!();
        let should_download = Confirm::new()
            .with_prompt("Download all documents now?")
            .default(true)
            .interact()?;

        if !should_download {
            println!();
            println!("{}", style("Project initialized without downloading documents.").yellow());
            println!();
            println!("{}", style("Run 'teamturbo pull' to download documents later.").dim());
            return Ok(());
        }
    }

    // Create empty category directories from category tree
    if let Some(ref category_tree) = docuram_config.category_tree {
        println!("{}", style("Creating category directories...").bold());
        let created_count = create_category_directories(category_tree, "docuram")?;
        if created_count > 0 {
            println!("{}", style(format!("✓ Created {} category director(ies)", created_count)).green());
        }
        println!();
    }

    println!();
    println!("{}", style("Downloading documents...").bold());
    println!();

    // Create progress bar
    let pb = ProgressBar::new(total_docs as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .expect("Invalid progress bar template")
            .progress_chars("=> ")
    );

    // Initialize local state
    let mut local_state = LocalState::default();

    // Download all documents (working documents + dependencies)
    let mut success_count = 0;
    let mut failed_docs = Vec::new();

    for doc_info in docuram_config.all_documents() {
        pb.set_message(format!("{}", doc_info.title));

        match download_document(&client, doc_info, &mut local_state).await {
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
        println!("{}", style(format!("✓ Successfully downloaded {} documents", success_count)).green());
    } else {
        println!("{}", style(format!("✓ Downloaded {} documents", success_count)).green());
        println!("{}", style(format!("✗ Failed to download {} documents:", failed_docs.len())).red());
        for (slug, error) in failed_docs {
            println!("  - {}: {}", slug, error);
        }
    }

    println!();
    println!("{}", style("Project initialized successfully!").green().bold());
    println!();
    println!("{}", style("You can now:").dim());
    println!("  {} {}", style("teamturbo pull").dim(), style("- Update documents").dim());
    println!("  {} {}", style("teamturbo push").dim(), style("- Push changes").dim());
    println!("  {} {}", style("teamturbo diff").dim(), style("- View changes").dim());

    Ok(())
}

/// Extract server URL from config URL
fn extract_server_url(config_url: &str) -> Result<String> {
    let url = url::Url::parse(config_url)
        .context("Invalid config URL")?;

    let scheme = url.scheme();
    let host = url.host_str().context("No host in URL")?;
    let port = url.port();

    let mut server_url = if let Some(port) = port {
        format!("{}://{}:{}", scheme, host, port)
    } else {
        format!("{}://{}", scheme, host)
    };

    // Development mode: map frontend ports to backend ports
    // Only map if the URL path indicates it's a frontend URL (no /api/ prefix)
    if (host == "127.0.0.1" || host == "localhost") && port.is_some() {
        let path = url.path();
        let is_api_request = path.starts_with("/api/") || path.starts_with("/docuram/") || path.starts_with("/cli/");

        // Only do port mapping for non-API requests
        if !is_api_request {
            let frontend_port = port.unwrap();
            let backend_port = match frontend_port {
                3100 => 3001,  // Standard Vite frontend -> Rails backend
                _ => frontend_port,  // Unknown port or already backend port, keep as is
            };

            if backend_port != frontend_port {
                server_url = format!("{}://{}:{}", scheme, host, backend_port);
            }
        }
    }

    Ok(server_url)
}

/// Download a single document
async fn download_document(
    client: &ApiClient,
    doc_info: &DocumentInfo,
    local_state: &mut LocalState,
) -> Result<()> {
    // Download document content
    logger::debug("download", &format!("Fetching document: {}", doc_info.uuid));
    let doc = client.download_document(&doc_info.uuid).await?;

    let mut content = doc.content.unwrap_or_default();
    logger::debug("download", &format!("Document size: {} bytes", content.len()));

    // Add docuram metadata to content
    content = add_docuram_metadata(&content, doc_info)?;

    // Write to file
    let file_path = PathBuf::from(&doc_info.path);
    write_file(&file_path, &content)
        .with_context(|| format!("Failed to write document to {:?}", file_path))?;
    logger::debug("download", &format!("Saved to: {:?}", file_path));

    // Calculate checksum of the actual file content (with metadata)
    let actual_checksum = calculate_checksum(&content);

    // Update local state
    local_state.upsert_document(crate::utils::storage::LocalDocumentInfo {
        uuid: doc_info.uuid.clone(),
        path: doc_info.path.clone(),
        checksum: actual_checksum,
        version: doc_info.version,
        last_sync: chrono::Utc::now().to_rfc3339(),
        pending_deletion: false,
    });

    Ok(())
}

/// Add docuram metadata to document content
fn add_docuram_metadata(content: &str, doc_info: &DocumentInfo) -> Result<String> {
    // Check if metadata already exists
    if content.starts_with("---\ndocuram:") || content.starts_with("---\r\ndocuram:") {
        logger::debug("metadata", "Document already has docuram metadata, skipping");
        return Ok(content.to_string());
    }

    // Build metadata frontmatter
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
  synced_at: "{}"
---

"#,
        doc_info.uuid,
        doc_info.title.replace('"', "\\\""),
        doc_info.category_path.replace('"', "\\\""),
        doc_info.category_uuid,
        doc_info.doc_type,
        doc_info.version,
        chrono::Utc::now().to_rfc3339()
    );

    // Prepend metadata to content
    Ok(format!("{}{}", metadata, content))
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
