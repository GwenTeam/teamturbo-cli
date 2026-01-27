use anyhow::{Context, Result};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::{Path, PathBuf};
use dialoguer::Confirm;

use crate::api::{ApiClient, PublicApiClient};
use crate::config::{CliConfig, DocuramConfig, DocumentInfo, PublicDependency};
use crate::utils::{write_file, logger, calculate_checksum};

pub async fn execute(config_url: Option<String>, force: bool, no_download: bool) -> Result<()> {
    println!("{}", style("Initialize Docuram Project").cyan().bold());
    println!();

    // Check if docuram.json already exists
    let config_path = Path::new("docuram.json");
    if config_path.exists() && !force {
        anyhow::bail!(
            "docuram.json already exists. Use --force to overwrite, or run 'teamturbo pull' to update documents."
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
    let api_config = client.get_docuram_config(&config_source).await?;

    // Ensure docuram directory exists
    fs::create_dir_all("docuram")
        .context("Failed to create docuram directory")?;

    // Save docuram.json
    println!("Saving {}...", style("docuram.json").cyan());
    let config_json = serde_json::to_string_pretty(&api_config)
        .context("Failed to serialize config")?;
    fs::write(&config_path, config_json)
        .context("Failed to write docuram.json")?;

    println!("{}", style("✓ Configuration saved").green());
    println!();

    // Reload config as our local DocuramConfig type (with local state fields)
    let docuram_config = DocuramConfig::load()
        .context("Failed to reload docuram.json")?;

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

    // Create standard docuram subdirectories
    println!("{}", style("Creating standard directories...").bold());
    let mut created_count = 0;

    // Standard subdirectories (organic, req, impl, manual)
    let standard_dirs = vec![
        ("organic", "User-maintained natural language documents (req*.md, bug*.md)"),
        ("req", "AI Agent and user-maintained extended requirement documents"),
        ("impl", "Implementation documents for each development iteration"),
        ("manual", "User manuals and operation guides"),
    ];

    for (dir_name, _description) in &standard_dirs {
        let dir_path = PathBuf::from("docuram").join(dir_name);
        if !dir_path.exists() {
            fs::create_dir_all(&dir_path)
                .with_context(|| format!("Failed to create {} directory", dir_name))?;
            logger::debug("create_dir", &format!("Created directory: {:?}", dir_path));
            created_count += 1;
        }
    }

    // Create dependencies directory (at project root) if there are dependency documents
    if !docuram_config.requires.is_empty() {
        let dependencies_path = PathBuf::from("dependencies");
        if !dependencies_path.exists() {
            fs::create_dir_all(&dependencies_path)
                .context("Failed to create dependencies directory")?;
            logger::debug("create_dir", &format!("Created dependencies directory: {:?}", dependencies_path));
            created_count += 1;
        }
    }

    if created_count > 0 {
        println!("{}", style(format!("✓ Created {} director(ies)", created_count)).green());
    }
    println!();

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

    // Make docuram_config mutable for updating local state fields
    let mut docuram_config = docuram_config;

    // Download all documents (working documents + dependencies)
    let mut success_count = 0;
    let mut failed_docs = Vec::new();

    // Collect UUIDs to download
    let uuids_to_download: Vec<String> = docuram_config.all_documents()
        .map(|d| d.uuid.clone())
        .collect();

    for doc_uuid in &uuids_to_download {
        let title = docuram_config.get_document_by_uuid(doc_uuid)
            .map(|d| d.title.clone())
            .unwrap_or_default();
        pb.set_message(format!("{}", title));

        let working_category_path = docuram_config.docuram.category_path.clone();
        match download_document(&client, doc_uuid, &mut docuram_config, &working_category_path).await {
            Ok(_) => {
                success_count += 1;
            }
            Err(e) => {
                failed_docs.push((doc_uuid.clone(), e.to_string()));
            }
        }

        pb.inc(1);
    }

    pb.finish_with_message("Done");

    // Save docuram config with updated local state fields
    docuram_config.save()
        .context("Failed to save docuram.json")?;

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

    // Fetch and download public dependencies from docuram.teamturbo.io
    println!();
    fetch_public_dependencies(&mut docuram_config).await?;

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
    doc_uuid: &str,
    docuram_config: &mut DocuramConfig,
    working_category_path: &str,
) -> Result<()> {
    // Download document content
    logger::debug("download", &format!("Fetching document: {}", doc_uuid));
    let doc = client.download_document(doc_uuid).await?;

    let content = doc.content.unwrap_or_default();
    logger::debug("download", &format!("Document size: {} bytes", content.len()));

    // Get document info to calculate local path
    let doc_info = docuram_config.get_document_by_uuid(doc_uuid)
        .context("Document not found in config")?;
    let local_file_path = doc_info.local_path(working_category_path);
    let file_path = PathBuf::from(&local_file_path);

    write_file(&file_path, &content)
        .with_context(|| format!("Failed to write document to {:?}", file_path))?;
    logger::debug("download", &format!("Saved to: {:?}", file_path));

    // Calculate checksum of the actual file content
    let actual_checksum = calculate_checksum(&content);

    // Update document's local state in docuram config
    if let Some(doc_mut) = docuram_config.get_document_by_uuid_mut(doc_uuid) {
        doc_mut.local_checksum = Some(actual_checksum);
        doc_mut.last_sync = Some(chrono::Utc::now().to_rfc3339());
        doc_mut.version = doc.version;
        doc_mut.pending_deletion = false;
    }

    Ok(())
}

/// Fetch and download public dependencies from docuram.teamturbo.io
async fn fetch_public_dependencies(docuram_config: &mut DocuramConfig) -> Result<()> {
    println!("{}", style("Fetching public dependencies from Docuram Official...").bold());

    let public_client = PublicApiClient::new(PublicApiClient::default_url().to_string());

    // Fetch global dependencies list
    let global_deps = match public_client.get_global_dependencies().await {
        Ok(deps) => deps,
        Err(e) => {
            println!("{}", style(format!("⚠ Could not fetch public dependencies: {}", e)).yellow());
            println!("{}", style("  (This is optional - your project will work without public dependencies)").dim());
            return Ok(());
        }
    };

    if global_deps.global_dependencies.is_empty() {
        println!("{}", style("  No public dependencies available").dim());
        return Ok(());
    }

    println!("{}", style(format!("Found {} public dependency categor(ies)", global_deps.global_dependencies.len())).dim());

    // Use dependencies directory for public dependencies as well
    let deps_dir = PathBuf::from("dependencies");
    if !deps_dir.exists() {
        fs::create_dir_all(&deps_dir)
            .context("Failed to create dependencies directory")?;
    }

    let mut total_docs_downloaded = 0;
    let mut public_deps_list: Vec<PublicDependency> = Vec::new();

    for dep_category in &global_deps.global_dependencies {
        println!();
        println!("{}", style(format!("Downloading: {} ({} documents)", dep_category.name, dep_category.document_count)).cyan());

        // Download the dependency's documents
        let download_result = match public_client.download_global_dependency(&dep_category.uuid).await {
            Ok(result) => result,
            Err(e) => {
                println!("{}", style(format!("  ⚠ Failed to download {}: {}", dep_category.name, e)).yellow());
                continue;
            }
        };

        // Create directory for this dependency
        let dep_dir = deps_dir.join(&dep_category.path);
        if !dep_dir.exists() {
            fs::create_dir_all(&dep_dir)
                .with_context(|| format!("Failed to create directory for {}", dep_category.name))?;
        }

        // Download each document
        let mut dep_documents: Vec<DocumentInfo> = Vec::new();
        for doc in &download_result.documents {
            let content = doc.content.clone().unwrap_or_default();

            // Build local path: dependencies/<category_path>/<subcategory>/<filename>
            let relative_path = doc.path.strip_prefix("docuram/").unwrap_or(&doc.path);
            let local_path = deps_dir.join(relative_path);

            // Ensure parent directory exists
            if let Some(parent) = local_path.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }

            // Write the document
            write_file(&local_path, &content)
                .with_context(|| format!("Failed to write document: {:?}", local_path))?;

            // Calculate checksum
            let checksum = calculate_checksum(&content);

            // Create DocumentInfo for this public dependency document
            let doc_info = DocumentInfo {
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
            };
            dep_documents.push(doc_info);
            total_docs_downloaded += 1;
        }

        // Create PublicDependency entry
        let public_dep = PublicDependency {
            category_uuid: dep_category.uuid.clone(),
            category_name: dep_category.name.clone(),
            category_path: dep_category.path.clone(),
            source_url: global_deps.source.url.clone(),
            document_count: dep_documents.len() as i64,
            documents: dep_documents,
        };
        public_deps_list.push(public_dep);

        println!("{}", style(format!("  ✓ Downloaded {} documents", download_result.documents.len())).green());
    }

    // Update docuram config with public dependencies
    docuram_config.public_dependencies = public_deps_list;
    docuram_config.save()
        .context("Failed to save docuram.json with public dependencies")?;

    println!();
    println!("{}", style(format!("✓ Downloaded {} public dependency document(s)", total_docs_downloaded)).green());

    Ok(())
}