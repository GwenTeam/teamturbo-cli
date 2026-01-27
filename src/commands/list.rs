use anyhow::Result;
use console::style;
use std::path::Path;
use std::collections::{HashSet, HashMap};
use walkdir::WalkDir;
use crate::config::{DocuramConfig, CliConfig};
use crate::utils;
use crate::api::{ApiClient, PublicApiClient};

/// Simple struct representing a new local document
struct NewLocalDocument {
    file_path: String,
    title: String,
}

/// Scan docuram/ directory for markdown files
fn scan_markdown_files(dir: &str) -> Result<Vec<NewLocalDocument>> {
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

        // Extract title from filename (without .md extension for display)
        let title = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string();

        documents.push(NewLocalDocument {
            file_path: path.to_string_lossy().to_string(),
            title,
        });
    }

    Ok(documents)
}

pub async fn execute() -> Result<()> {
    println!("{}", style("Document List").cyan().bold());
    println!();

    // Load docuram config with migration from state.json
    let mut docuram_config = DocuramConfig::load_with_migration()?;

    // Get working category path
    let working_category_path = &docuram_config.docuram.category_path.clone();

    // Auto-detect missing files and mark them as pending deletion
    let mut config_changed = false;
    for doc in docuram_config.all_documents_mut() {
        // Only check documents that have been synced (have local_checksum)
        if !doc.pending_deletion && doc.local_checksum.is_some() {
            let local_file_path = doc.local_path(working_category_path);
            let file_path = Path::new(&local_file_path);
            if !file_path.exists() {
                // File is missing - mark for deletion
                doc.pending_deletion = true;
                config_changed = true;
                println!("{} File missing, marked for deletion: {}",
                    style("⚠").yellow(), local_file_path);
            }
        }
    }

    // Save config if any changes were made
    if config_changed {
        docuram_config.save()?;
        println!();
    }

    // Try to fetch remote documents and versions
    let (remote_versions, remote_docs) = fetch_remote_documents(&docuram_config).await;

    // Scan docuram directory for new local documents (by comparing files vs JSON)
    let new_local_docs = match scan_markdown_files("docuram") {
        Ok(docs) => {
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
            docs.into_iter()
                .filter(|d| {
                    let in_docuram = docuram_paths.contains(&d.file_path);
                    let in_local = local_doc_paths.contains(&d.file_path);

                    // Document is new if not found by path in either array
                    !in_docuram && !in_local
                })
                .collect::<Vec<_>>()
        }
        Err(_) => Vec::new()
    };

    // Print project info
    println!("{}", style(format!("Project: {} ({})", docuram_config.project.name, docuram_config.project.url)).bold());
    println!();

    // Collect all documents with their status
    let all_docs: Vec<_> = docuram_config.all_documents().collect();

    // Collect pending deletion documents (they should still be displayed with special styling)
    let pending_deletion_docs: Vec<_> = all_docs.iter()
        .filter(|doc| doc.pending_deletion)
        .collect();

    // No more state_only_docs since all synced documents are now in docuram.json
    let state_only_docs: Vec<&crate::config::DocumentInfo> = Vec::new();

    // Find remote documents that are not in local docuram.json
    let mut remote_new_docs = Vec::new();
    if let Ok(ref remote_doc_list) = remote_docs {
        let local_uuids: HashSet<String> = all_docs.iter()
            .map(|d| d.uuid.clone())
            .collect();

        for remote_doc in remote_doc_list {
            // Skip if in docuram.json
            if local_uuids.contains(&remote_doc.uuid) {
                continue;
            }

            // Skip if marked for deletion (already in docuram.json)
            if let Some(local_doc) = docuram_config.get_document_by_uuid(&remote_doc.uuid) {
                if local_doc.pending_deletion {
                    continue;
                }
            }

            remote_new_docs.push(remote_doc.clone());
        }
    }

    // Note: pending_deletion_docs are already included in all_docs, so don't double-count
    let total_count = all_docs.len() + new_local_docs.len() + remote_new_docs.len();
    if total_count == 0 {
        println!("{}", style("No documents found").yellow());
        return Ok(());
    }

    println!("{}", style(format!("Total documents: {} ({} in docuram.json, {} new local, {} new on server, {} pending deletion)",
        total_count, all_docs.len(), new_local_docs.len(), remote_new_docs.len(), pending_deletion_docs.len())).bold());
    println!();

    // Build a tree structure grouped by category
    let mut tree: HashMap<String, Vec<ListDocumentInfo>> = HashMap::new();

    // Group documents by actual file directory path (not category_path)
    for doc in &all_docs {
        // Use the path stored in docuram.json (was migrated from state.json if applicable)
        let actual_file_path = doc.local_path(working_category_path);

        // Extract directory path from actual file path (preserve full path for tree display)
        let file_path = Path::new(&actual_file_path);
        let dir_path = if let Some(parent) = file_path.parent() {
            parent.to_str().unwrap_or("Unknown").to_string()
        } else {
            "Unknown".to_string()
        };

        // Get actual filename from file path (preserving case)
        let actual_filename = file_path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&doc.title)
            .to_string();

        tree.entry(dir_path)
            .or_insert_with(Vec::new)
            .push(ListDocumentInfo {
                title: actual_filename,
                uuid: doc.uuid.clone(),
                doc_type: doc.doc_type.clone(),
                status: get_document_status_from_doc(doc, &actual_file_path),
                local_version: get_local_version_from_doc(doc),
                remote_version: get_remote_version(&doc.uuid, &remote_versions),
                source: DocumentSource::Docuram,
                is_public: false,
            });
    }

    // state_only_docs is now empty since all synced documents are in docuram.json
    // Keep the loop for compatibility, but it won't execute
    for state_doc in &state_only_docs {
        let file_path = Path::new(&state_doc.path);
        let title = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string();

        let category = if let Some(parent) = file_path.parent() {
            parent.to_str().unwrap_or("Unknown").to_string()
        } else {
            "Unknown".to_string()
        };

        tree.entry(category)
            .or_insert_with(Vec::new)
            .push(ListDocumentInfo {
                title,
                uuid: state_doc.uuid.clone(),
                doc_type: "?".to_string(),
                status: "Synced".to_string(),
                local_version: state_doc.version.to_string(),
                remote_version: get_remote_version(&state_doc.uuid, &remote_versions),
                source: DocumentSource::StateOnly,
                is_public: false,
            });
    }

    // Add new local documents
    for new_doc in &new_local_docs {
        // Extract directory path from the actual file path (preserve full path)
        let file_path = Path::new(&new_doc.file_path);
        let dir_path = if let Some(parent) = file_path.parent() {
            parent.to_str().unwrap_or("Unknown").to_string()
        } else {
            "Unknown".to_string()
        };

        // Get original filename without extension (preserving case)
        let original_filename = file_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string();

        tree.entry(dir_path)
            .or_insert_with(Vec::new)
            .push(ListDocumentInfo {
                title: original_filename,
                uuid: String::new(),
                doc_type: "knowledge".to_string(),
                status: "New".to_string(),
                local_version: "-".to_string(),
                remote_version: "-".to_string(),
                source: DocumentSource::New,
                is_public: false,
            });
    }

    // Add remote new documents (on server but not in local docuram.json)
    for remote_doc in &remote_new_docs {
        // Use local_path() to get correct path (dependencies go in dependencies/ at project root)
        let local_file_path = remote_doc.local_path(working_category_path);

        // Extract directory path from local file path (preserve full path)
        let file_path = Path::new(&local_file_path);
        let dir_path = if let Some(parent) = file_path.parent() {
            parent.to_str().unwrap_or("Unknown").to_string()
        } else {
            "Unknown".to_string()
        };

        tree.entry(dir_path)
            .or_insert_with(Vec::new)
            .push(ListDocumentInfo {
                title: remote_doc.title.clone(),
                uuid: remote_doc.uuid.clone(),
                doc_type: remote_doc.doc_type.clone(),
                status: "Remote".to_string(),
                local_version: "-".to_string(),
                remote_version: remote_doc.version.to_string(),
                source: DocumentSource::Remote,
                is_public: false,
            });
    }

    // Pending deletion documents are already included in all_docs
    // The status is determined by get_document_status_from_doc() based on pending_deletion flag
    // No need to add them separately

    // Fetch remote public dependencies for version comparison
    let (public_remote_versions, public_new_docs) = fetch_public_dependencies_info(&docuram_config).await;

    // Build set of existing public doc UUIDs
    let existing_public_uuids: HashSet<String> = docuram_config.public_dependencies
        .iter()
        .flat_map(|dep| dep.documents.iter().map(|d| d.uuid.clone()))
        .collect();

    // Add public dependencies from public_dependencies array
    for public_dep in &docuram_config.public_dependencies {
        for doc in &public_dep.documents {
            // Extract directory path from document path
            let file_path = Path::new(&doc.path);
            let dir_path = if let Some(parent) = file_path.parent() {
                parent.to_str().unwrap_or("dependencies").to_string()
            } else {
                "dependencies".to_string()
            };

            // Get actual filename from file path
            let actual_filename = file_path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&doc.title)
                .to_string();

            // Check document status
            let status = get_document_status_from_doc(doc, &doc.path);

            // Get remote version for public dependency
            let remote_ver = public_remote_versions
                .get(&doc.uuid)
                .map(|v| v.to_string())
                .unwrap_or_else(|| doc.version.to_string());

            tree.entry(dir_path)
                .or_insert_with(Vec::new)
                .push(ListDocumentInfo {
                    title: actual_filename,
                    uuid: doc.uuid.clone(),
                    doc_type: doc.doc_type.clone(),
                    status,
                    local_version: doc.version.to_string(),
                    remote_version: remote_ver,
                    source: DocumentSource::Docuram,
                    is_public: true,
                });
        }
    }

    // Add new public documents (on remote but not local)
    for (category_path, doc) in &public_new_docs {
        if existing_public_uuids.contains(&doc.uuid) {
            continue;
        }

        let dir_path = format!("dependencies/{}/{}", category_path, doc.category_name);

        tree.entry(dir_path)
            .or_insert_with(Vec::new)
            .push(ListDocumentInfo {
                title: doc.title.clone(),
                uuid: doc.uuid.clone(),
                doc_type: doc.doc_type.clone(),
                status: "Remote".to_string(),
                local_version: "-".to_string(),
                remote_version: doc.version.to_string(),
                source: DocumentSource::Remote,
                is_public: true,
            });
    }

    // No longer add empty categories from category_tree
    // We only show document type directories (docuram/organic, docuram/impl, etc.) with actual content

    // Ensure standard directories are always shown (docuram/organic, docuram/impl, docuram/req, docuram/manual) even if empty
    for standard_dir in ["docuram/organic", "docuram/impl", "docuram/req", "docuram/manual"] {
        if !tree.contains_key(standard_dir) {
            tree.insert(standard_dir.to_string(), Vec::new());
        }
    }

    // Build hierarchical tree structure
    let tree_structure = build_tree_structure(&tree);

    // Print tree
    println!("{}", style("Document Tree:").bold());
    println!();

    print_tree_node(&tree_structure, &tree, "", true);

    println!();
    println!("{}", style("Legend:").bold());
    println!("  {} - File synced and unchanged", style("✓ Synced").green());
    println!("  {} - File has local modifications", style("⚠ Modified").yellow());
    println!("  {} - Error reading file", style("✗ Error").red());
    println!("  {} - File not downloaded yet", style("○ Not downloaded").dim());
    println!("  {} - New local document (run 'teamturbo push' to upload)", style("+ New").cyan().bold());
    println!("  {} - New document on server (run 'teamturbo pull' to download)", style("⬇ Remote").blue().bold());
    println!("  {} - File deleted, pending server sync (run 'teamturbo push' to delete from server)", style("🗑 Pending deletion").red().dim());
    println!("  {} - Remote version has updates available", style("[v1→v2]").yellow());
    println!("  {} - Public dependency from docuram.teamturbo.io", style("[PUBLIC]").magenta().bold());
    println!();

    Ok(())
}

// Helper structures
struct ListDocumentInfo {
    title: String,
    uuid: String,
    doc_type: String,
    status: String,
    local_version: String,
    remote_version: String,
    source: DocumentSource,
    is_public: bool,
}

enum DocumentSource {
    Docuram,
    StateOnly,
    New,
    Remote,
}

// Helper functions
fn get_document_status_from_doc(doc: &crate::config::DocumentInfo, path: &str) -> String {
    // Check if marked for deletion first
    if doc.pending_deletion {
        return "Pending deletion".to_string();
    }

    // Check if document has been synced (has local_checksum)
    if let Some(local_checksum) = &doc.local_checksum {
        let file_path = Path::new(path);
        if file_path.exists() {
            match utils::read_file(path) {
                Ok(content) => {
                    // Calculate checksum of complete content
                    let current_checksum = utils::calculate_checksum(&content);
                    if current_checksum == *local_checksum {
                        "Synced".to_string()
                    } else {
                        "Modified".to_string()
                    }
                }
                Err(_) => "Error".to_string(),
            }
        } else {
            // File is missing - treat as pending deletion
            "Pending deletion".to_string()
        }
    } else {
        let file_path = Path::new(path);
        if file_path.exists() {
            "Not synced".to_string()
        } else {
            "Not downloaded".to_string()
        }
    }
}

fn get_local_version_from_doc(doc: &crate::config::DocumentInfo) -> String {
    if doc.local_checksum.is_some() {
        doc.version.to_string()
    } else {
        "-".to_string()
    }
}

fn get_remote_version(uuid: &str, remote_versions: &Result<HashMap<String, i64>>) -> String {
    match remote_versions {
        Ok(versions_map) => {
            versions_map.get(uuid)
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string())
        },
        Err(_) => "-".to_string(),
    }
}

fn get_status_colored(status: &str) -> console::StyledObject<String> {
    match status {
        "Synced" => style(format!("✓ {}", status)).green(),
        "Modified" => style(format!("⚠ {}", status)).yellow(),
        "Error" => style(format!("✗ {}", status)).red(),
        "Not synced" => style(format!("⚠ {}", status)).yellow(),
        "Not downloaded" => style(format!("○ {}", status)).dim(),
        "New" => style(format!("+ {}", status)).cyan().bold(),
        "Remote" => style(format!("⬇ {}", status)).blue().bold(),
        "Pending deletion" => style(format!("🗑 {}", status)).red().dim().strikethrough(),
        _ => style(status.to_string()).white(),
    }
}

fn format_version_info(local_version: &str, remote_version: &str) -> console::StyledObject<String> {
    if local_version != "-" && remote_version != "-" {
        let local_ver: i64 = local_version.parse().unwrap_or(0);
        let remote_ver: i64 = remote_version.parse().unwrap_or(0);
        if remote_ver > local_ver {
            style(format!("[v{}→v{}]", local_ver, remote_ver)).yellow()
        } else {
            style(format!("[v{}]", local_ver)).dim()
        }
    } else if local_version != "-" {
        style(format!("[v{}]", local_version)).dim()
    } else {
        style("".to_string()).dim()
    }
}

// Tree structure for hierarchical display
#[derive(Debug)]
struct TreeNode {
    path: String,
    children: Vec<TreeNode>,
}

/// Build hierarchical tree structure from flat category paths
fn build_tree_structure(tree: &HashMap<String, Vec<ListDocumentInfo>>) -> Vec<TreeNode> {
    let mut all_paths: Vec<String> = tree.keys().cloned().collect();
    all_paths.sort();

    let mut root_nodes: Vec<TreeNode> = Vec::new();

    for path in &all_paths {
        insert_into_tree(&mut root_nodes, path, &all_paths);
    }

    root_nodes
}

/// Insert a path into the tree structure, creating intermediate nodes as needed
fn insert_into_tree(nodes: &mut Vec<TreeNode>, path: &str, _all_paths: &[String]) {
    insert_into_tree_recursive(nodes, path, 0);
}

/// Helper function to recursively insert a path into the tree
fn insert_into_tree_recursive(nodes: &mut Vec<TreeNode>, path: &str, depth: usize) {
    let parts: Vec<&str> = path.split('/').collect();

    if depth >= parts.len() {
        return;
    }

    // Build the path up to current depth
    let current_path = parts[..=depth].join("/");

    // Check if this node already exists
    let existing_idx = nodes.iter().position(|n| n.path == current_path);

    if let Some(idx) = existing_idx {
        // Node exists, recurse into its children
        insert_into_tree_recursive(&mut nodes[idx].children, path, depth + 1);
    } else {
        // Node doesn't exist, create it
        let mut new_node = TreeNode {
            path: current_path,
            children: Vec::new(),
        };

        // If there are more parts, recurse to create children
        if depth + 1 < parts.len() {
            insert_into_tree_recursive(&mut new_node.children, path, depth + 1);
        }

        nodes.push(new_node);
    }
}

/// Try to insert path as a child of parent_path
fn try_insert_as_child(nodes: &mut Vec<TreeNode>, path: &str, parent_path: &str) -> bool {
    for node in nodes.iter_mut() {
        if node.path == parent_path {
            // Found parent, add as child if not already present
            if !node.children.iter().any(|c| c.path == path) {
                node.children.push(TreeNode {
                    path: path.to_string(),
                    children: Vec::new(),
                });
            }
            return true;
        }
        // Recursively search in children
        if try_insert_as_child(&mut node.children, path, parent_path) {
            return true;
        }
    }
    false
}

/// Get parent path from a category path
fn get_parent_path(path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() > 1 {
        Some(parts[..parts.len() - 1].join("/"))
    } else {
        None
    }
}

/// Print tree node recursively
fn print_tree_node(
    nodes: &[TreeNode],
    tree: &HashMap<String, Vec<ListDocumentInfo>>,
    prefix: &str,
    is_root: bool,
) {
    for (idx, node) in nodes.iter().enumerate() {
        let is_last = idx == nodes.len() - 1;

        // Print category - extract just the directory name from the full path
        let category_prefix = if is_last { "└──" } else { "├──" };
        let dir_name = node.path.split('/').last().unwrap_or(&node.path);

        println!("{}{} {} {}",
            prefix,
            style(category_prefix).dim(),
            style("📁").cyan(),
            style(dir_name).bold().cyan()
        );

        // Build prefix for this node's content (documents and children)
        let node_prefix = if is_last {
            format!("{}   ", prefix)
        } else {
            format!("{}│  ", prefix)
        };

        // Print documents in this category
        if let Some(docs) = tree.get(&node.path) {
            let has_children = !node.children.is_empty();

            if docs.is_empty() && !has_children {
                // Empty directory with no children
                println!("{}   {}", node_prefix, style("(empty)").dim().italic());
            } else {
                for (doc_idx, doc) in docs.iter().enumerate() {
                    let is_last_doc = doc_idx == docs.len() - 1 && !has_children;
                    let doc_prefix = if is_last_doc { "└──" } else { "├──" };

                    // Format document line
                    let status_colored = get_status_colored(&doc.status);
                    let version_info = format_version_info(&doc.local_version, &doc.remote_version);

                    // Apply strikethrough to title if pending deletion
                    let title_styled = if doc.status == "Pending deletion" {
                        style(&doc.title).dim().strikethrough()
                    } else {
                        style(&doc.title).white()
                    };

                    // Add [PUBLIC] marker for public dependencies
                    let public_marker = if doc.is_public {
                        style("[PUBLIC]").magenta().bold()
                    } else {
                        style("").white()
                    };

                    println!("{}{} {} {} {} {} {}",
                        node_prefix,
                        style(doc_prefix).dim(),
                        style("📄").dim(),
                        title_styled,
                        public_marker,
                        status_colored,
                        version_info
                    );
                }
            }
        }

        // Print children categories
        if !node.children.is_empty() {
            print_tree_node(&node.children, tree, &node_prefix, false);
        }

        // Print vertical line between root categories
        if is_root && !is_last {
            println!("{}", style("│").dim());
        }
    }
}

/// Fetch remote documents and versions from server
async fn fetch_remote_documents(docuram_config: &DocuramConfig) -> (Result<HashMap<String, i64>>, Result<Vec<crate::api::client::DocumentInfo>>) {
    // Load CLI config
    let cli_config = match CliConfig::load() {
        Ok(config) => config,
        Err(e) => return (Err(e.into()), Err(anyhow::anyhow!("Failed to load CLI config"))),
    };

    let server_url = docuram_config.server_url();

    // Get auth for this server
    let auth = match cli_config.get_auth(server_url) {
        Some(auth) => auth,
        None => {
            let err_msg = format!("Not logged in to {}. Showing local versions only.", server_url);
            return (Err(anyhow::anyhow!("{}", err_msg)), Err(anyhow::anyhow!("{}", err_msg)));
        }
    };

    // Create API client
    let client = ApiClient::new(server_url.to_string(), auth.access_token.clone());

    // Get category UUID from docuram config
    let category_uuid = match &docuram_config.docuram.category_uuid {
        Some(uuid) => uuid,
        None => {
            let err_msg = "No category UUID in docuram.json";
            return (Err(anyhow::anyhow!("{}", err_msg)), Err(anyhow::anyhow!("{}", err_msg)));
        }
    };

    // Fetch document versions
    let remote_docs = match client.get_document_versions(category_uuid).await {
        Ok(docs) => docs,
        Err(e) => return (Err(e.into()), Err(anyhow::anyhow!("Failed to fetch remote documents"))),
    };

    // Convert to HashMap for easy lookup
    let versions_map: HashMap<String, i64> = remote_docs
        .iter()
        .map(|doc| (doc.uuid.clone(), doc.version))
        .collect();

    (Ok(versions_map), Ok(remote_docs))
}

// Function removed - no longer needed with the new flat document type structure
// We only show directories that actually contain documents
//
// /// Recursively add empty categories to the tree
// fn add_empty_categories_to_tree(
//     tree: &mut HashMap<String, Vec<DocumentInfo>>,
//     category: &crate::config::CategoryTree,
//     _parent_path: &str,
// ) {
//     // Use the full path from the category tree
//     let current_path = &category.path;
//
//     // If this category has no documents and is not already in the tree, add it as empty
//     if category.document_count == 0 && !tree.contains_key(current_path) {
//         tree.insert(current_path.clone(), Vec::new());
//     }
//
//     // Recursively process subcategories
//     if let Some(ref subcategories) = category.subcategories {
//         for subcat in subcategories {
//             add_empty_categories_to_tree(tree, subcat, "");
//         }
//     }
// }

/// Public dependency document info for list display
struct PublicDocInfo {
    uuid: String,
    title: String,
    category_name: String,
    doc_type: String,
    version: i64,
}

/// Fetch public dependencies info from docuram.teamturbo.io
/// Returns: (version map, list of new documents with their category paths)
async fn fetch_public_dependencies_info(docuram_config: &DocuramConfig) -> (HashMap<String, i64>, Vec<(String, PublicDocInfo)>) {
    let public_client = PublicApiClient::new(PublicApiClient::default_url().to_string());

    // Fetch global dependencies list
    let global_deps = match public_client.get_global_dependencies().await {
        Ok(deps) => deps,
        Err(_) => return (HashMap::new(), Vec::new()),
    };

    let mut version_map: HashMap<String, i64> = HashMap::new();
    let mut new_docs: Vec<(String, PublicDocInfo)> = Vec::new();

    // Build set of existing local public doc UUIDs
    let existing_uuids: HashSet<String> = docuram_config.public_dependencies
        .iter()
        .flat_map(|dep| dep.documents.iter().map(|d| d.uuid.clone()))
        .collect();

    for dep_category in &global_deps.global_dependencies {
        // Download the dependency's documents to get versions
        let download_result = match public_client.download_global_dependency(&dep_category.uuid).await {
            Ok(result) => result,
            Err(_) => continue,
        };

        for doc in &download_result.documents {
            // Add to version map
            version_map.insert(doc.uuid.clone(), doc.version);

            // Check if this is a new document
            if !existing_uuids.contains(&doc.uuid) {
                new_docs.push((dep_category.path.clone(), PublicDocInfo {
                    uuid: doc.uuid.clone(),
                    title: doc.title.clone(),
                    category_name: doc.category_name.clone(),
                    doc_type: doc.doc_type.clone(),
                    version: doc.version,
                }));
            }
        }
    }

    (version_map, new_docs)
}
