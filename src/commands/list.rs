use anyhow::Result;
use console::style;
use std::path::Path;
use std::collections::{HashSet, HashMap};
use crate::config::{DocuramConfig, CliConfig};
use crate::utils::storage::LocalState;
use crate::utils;
use crate::api::ApiClient;

pub async fn execute() -> Result<()> {
    println!("{}", style("Document List").cyan().bold());
    println!();

    // Load docuram config
    let docuram_config = DocuramConfig::load()?;

    // Get working category path
    let working_category_path = &docuram_config.docuram.category_path;

    // Load local state
    let local_state = LocalState::load()?;

    // Try to fetch remote documents and versions
    let (remote_versions, remote_docs) = fetch_remote_documents(&docuram_config).await;

    // Scan docuram directory for new local documents with front matter
    let new_docs_with_meta = match utils::scan_documents_with_meta("docuram") {
        Ok(docs) => {
            // Build a set of file paths from docuram.json for quick lookup
            let docuram_paths: HashSet<String> = docuram_config
                .all_documents()
                .map(|d| d.path.clone())
                .collect();

            // Build a set of file paths from state.json
            let state_paths: HashSet<String> = local_state
                .documents
                .values()
                .map(|doc_info| doc_info.path.clone())
                .collect();

            // Build a set of UUIDs from docuram.json (if the frontmatter has uuid)
            let docuram_uuids: HashSet<String> = docuram_config
                .all_documents()
                .map(|d| d.uuid.clone())
                .collect();

            // Build a set of UUIDs from state.json
            let state_uuids: HashSet<String> = local_state
                .documents
                .keys()
                .cloned()
                .collect();

            // Filter: new documents are those NOT in docuram.json AND NOT in state.json
            docs.into_iter()
                .filter(|d| {
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
                .collect::<Vec<_>>()
        }
        Err(_) => Vec::new()
    };

    // Print project info
    println!("{}", style(format!("Project: {} ({})", docuram_config.project.name, docuram_config.project.url)).bold());
    println!();

    // Collect all documents with their status
    let all_docs: Vec<_> = docuram_config.all_documents().collect();

    // Collect documents that are in state.json but not in docuram.json
    // Also collect documents marked for deletion (they should still be displayed)
    let mut state_only_docs = Vec::new();
    let mut pending_deletion_docs = Vec::new();

    for (uuid, doc_info) in &local_state.documents {
        let in_docuram = all_docs.iter().any(|d| d.uuid == *uuid);

        if doc_info.pending_deletion {
            // Collect pending deletion documents separately
            pending_deletion_docs.push(doc_info.clone());
        } else if !in_docuram {
            // Not in docuram.json and not pending deletion
            let file_path = Path::new(&doc_info.path);
            if file_path.exists() {
                state_only_docs.push(doc_info.clone());
            }
        }
    }

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

            // Skip if marked for deletion in state.json
            if let Some(state_doc) = local_state.get_document(&remote_doc.uuid) {
                if state_doc.pending_deletion {
                    continue;
                }
            }

            remote_new_docs.push(remote_doc.clone());
        }
    }

    let total_count = all_docs.len() + new_docs_with_meta.len() + state_only_docs.len() + remote_new_docs.len() + pending_deletion_docs.len();
    if total_count == 0 {
        println!("{}", style("No documents found").yellow());
        return Ok(());
    }

    println!("{}", style(format!("Total documents: {} ({} in docuram.json, {} pushed but not in config, {} new local, {} new on server, {} pending deletion)",
        total_count, all_docs.len(), state_only_docs.len(), new_docs_with_meta.len(), remote_new_docs.len(), pending_deletion_docs.len())).bold());
    println!();

    // Build a tree structure grouped by category
    let mut tree: HashMap<String, Vec<DocumentInfo>> = HashMap::new();

    // Group documents by actual file directory path (not category_path)
    for doc in &all_docs {
        // Use local_path() to get correct path (dependencies go in working_category/dependencies/ subdirectory)
        let local_file_path = doc.local_path(working_category_path);

        // Extract directory path from local file path
        let file_path = Path::new(&local_file_path);
        let dir_path = if let Some(parent) = file_path.parent() {
            if let Some(parent_str) = parent.to_str() {
                parent_str.strip_prefix("docuram/").unwrap_or(parent_str).to_string()
            } else {
                "Unknown".to_string()
            }
        } else {
            "Unknown".to_string()
        };

        tree.entry(dir_path)
            .or_insert_with(Vec::new)
            .push(DocumentInfo {
                title: doc.title.clone(),
                uuid: doc.uuid.clone(),
                doc_type: doc.doc_type.clone(),
                status: get_document_status(&doc.uuid, &local_file_path, &local_state),
                local_version: get_local_version(&doc.uuid, &local_state),
                remote_version: get_remote_version(&doc.uuid, &remote_versions),
                source: DocumentSource::Docuram,
            });
    }

    // Add state-only documents
    for state_doc in &state_only_docs {
        let file_path = Path::new(&state_doc.path);
        let title = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string();

        let category = if let Some(parent) = file_path.parent() {
            if let Some(parent_str) = parent.to_str() {
                parent_str.strip_prefix("docuram/").unwrap_or(parent_str).to_string()
            } else {
                "Unknown".to_string()
            }
        } else {
            "Unknown".to_string()
        };

        tree.entry(category)
            .or_insert_with(Vec::new)
            .push(DocumentInfo {
                title,
                uuid: state_doc.uuid.clone(),
                doc_type: "?".to_string(),
                status: "Synced".to_string(),
                local_version: state_doc.version.to_string(),
                remote_version: get_remote_version(&state_doc.uuid, &remote_versions),
                source: DocumentSource::StateOnly,
            });
    }

    // Add new local documents
    for new_doc in &new_docs_with_meta {
        tree.entry(new_doc.front_matter.category.clone())
            .or_insert_with(Vec::new)
            .push(DocumentInfo {
                title: new_doc.front_matter.title.clone(),
                uuid: String::new(),
                doc_type: new_doc.front_matter.doc_type.clone().unwrap_or_else(|| "knowledge".to_string()),
                status: "New".to_string(),
                local_version: "-".to_string(),
                remote_version: "-".to_string(),
                source: DocumentSource::New,
            });
    }

    // Add remote new documents (on server but not in local docuram.json)
    for remote_doc in &remote_new_docs {
        // Use local_path() to get correct path (dependencies go in working_category/dependencies/ subdirectory)
        let local_file_path = remote_doc.local_path(working_category_path);

        // Extract directory path from local file path
        let file_path = Path::new(&local_file_path);
        let dir_path = if let Some(parent) = file_path.parent() {
            if let Some(parent_str) = parent.to_str() {
                parent_str.strip_prefix("docuram/").unwrap_or(parent_str).to_string()
            } else {
                "Unknown".to_string()
            }
        } else {
            "Unknown".to_string()
        };

        tree.entry(dir_path)
            .or_insert_with(Vec::new)
            .push(DocumentInfo {
                title: remote_doc.title.clone(),
                uuid: remote_doc.uuid.clone(),
                doc_type: remote_doc.doc_type.clone(),
                status: "Remote".to_string(),
                local_version: "-".to_string(),
                remote_version: remote_doc.version.to_string(),
                source: DocumentSource::Remote,
            });
    }

    // Add pending deletion documents
    for pending_doc in &pending_deletion_docs {
        let file_path = Path::new(&pending_doc.path);
        let title = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string();

        let category = if let Some(parent) = file_path.parent() {
            if let Some(parent_str) = parent.to_str() {
                parent_str.strip_prefix("docuram/").unwrap_or(parent_str).to_string()
            } else {
                "Unknown".to_string()
            }
        } else {
            "Unknown".to_string()
        };

        tree.entry(category)
            .or_insert_with(Vec::new)
            .push(DocumentInfo {
                title,
                uuid: pending_doc.uuid.clone(),
                doc_type: "?".to_string(),
                status: "Pending deletion".to_string(),
                local_version: pending_doc.version.to_string(),
                remote_version: get_remote_version(&pending_doc.uuid, &remote_versions),
                source: DocumentSource::StateOnly,
            });
    }

    // No longer add empty categories from category_tree
    // We only show document type directories (organic, impl, dependencies) with actual content

    // Ensure standard directories are always shown (organic, impl, req) even if empty
    for standard_dir in ["organic", "impl", "req"] {
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
    println!("  {} - File missing or has errors", style("✗ Missing/Error").red());
    println!("  {} - File not downloaded yet", style("○ Not downloaded").dim());
    println!("  {} - New local document (run 'teamturbo push' to upload)", style("+ New").cyan().bold());
    println!("  {} - New document on server (run 'teamturbo pull' to download)", style("⬇ Remote").blue().bold());
    println!("  {} - Marked for deletion (run 'teamturbo push' to delete from server)", style("🗑 Pending deletion").red().dim());
    println!("  {} - Remote version has updates available", style("[v1→v2]").yellow());
    println!();

    Ok(())
}

// Helper structures
struct DocumentInfo {
    title: String,
    uuid: String,
    doc_type: String,
    status: String,
    local_version: String,
    remote_version: String,
    source: DocumentSource,
}

enum DocumentSource {
    Docuram,
    StateOnly,
    New,
    Remote,
}

// Helper functions
fn get_document_status(uuid: &str, path: &str, local_state: &LocalState) -> String {
    if let Some(local_doc) = local_state.get_document(uuid) {
        // Check if marked for deletion first
        if local_doc.pending_deletion {
            return "Pending deletion".to_string();
        }

        let file_path = Path::new(path);
        if file_path.exists() {
            match utils::read_file(path) {
                Ok(content) => {
                    // Calculate checksum of complete content (including frontmatter)
                    let current_checksum = utils::calculate_checksum(&content);
                    if current_checksum == local_doc.checksum {
                        "Synced".to_string()
                    } else {
                        "Modified".to_string()
                    }
                }
                Err(_) => "Error".to_string(),
            }
        } else {
            "Missing".to_string()
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

fn get_local_version(uuid: &str, local_state: &LocalState) -> String {
    local_state.get_document(uuid)
        .map(|d| d.version.to_string())
        .unwrap_or_else(|| "-".to_string())
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
        "Error" | "Missing" => style(format!("✗ {}", status)).red(),
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
fn build_tree_structure(tree: &HashMap<String, Vec<DocumentInfo>>) -> Vec<TreeNode> {
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
    tree: &HashMap<String, Vec<DocumentInfo>>,
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

                    println!("{}{} {} {} {} {}",
                        node_prefix,
                        style(doc_prefix).dim(),
                        style("📄").dim(),
                        title_styled,
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
