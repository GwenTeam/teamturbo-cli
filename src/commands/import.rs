use anyhow::{Result, Context};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::fs;
use walkdir::WalkDir;

use crate::config::DocuramConfig;
use crate::utils::{update_front_matter, FrontMatter};

/// Import documents from a git repository or local directory
pub async fn execute(paths: Vec<String>, from: Option<String>, to: Option<String>) -> Result<()> {
    println!("{}", style("Import Documents").cyan().bold());
    println!();

    // Load docuram config to validate we're in a docuram project
    let _docuram_config = DocuramConfig::load()
        .context("Failed to load docuram.json. Run 'teamturbo init' first.")?;

    // Determine the import mode
    let import_mode = determine_import_mode(&paths, &from, &to)?;

    match import_mode {
        ImportMode::InPlace(in_place_paths) => {
            execute_in_place_import(in_place_paths).await
        }
        ImportMode::Remote { source, target_category } => {
            execute_remote_import(source, target_category).await
        }
    }
}

/// Import mode enum
enum ImportMode {
    /// In-place conversion of local files/directories
    InPlace(Vec<PathBuf>),
    /// Remote import from git or external directory to target category
    Remote { source: String, target_category: String },
}

/// Determine import mode based on arguments
fn determine_import_mode(paths: &[String], from: &Option<String>, to: &Option<String>) -> Result<ImportMode> {
    match (paths.is_empty(), from, to) {
        // Case 1: paths provided, no --from/--to -> in-place conversion
        (false, None, None) => {
            let path_bufs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
            // Validate all paths exist
            for path in &path_bufs {
                if !path.exists() {
                    anyhow::bail!("Path does not exist: {:?}", path);
                }
            }
            Ok(ImportMode::InPlace(path_bufs))
        }
        // Case 2: --from and --to provided -> remote import
        (true, Some(from_path), Some(to_path)) => {
            Ok(ImportMode::Remote {
                source: from_path.clone(),
                target_category: to_path.clone(),
            })
        }
        // Case 3: only --from provided, no paths -> in-place conversion of --from
        (true, Some(from_path), None) => {
            let path = PathBuf::from(from_path);
            if !path.exists() {
                anyhow::bail!("Source does not exist: {:?}", path);
            }
            Ok(ImportMode::InPlace(vec![path]))
        }
        // Invalid cases
        (false, Some(_), _) => {
            anyhow::bail!("Cannot use positional paths with --from. Use either:\n  - 'teamturbo import <paths...>' for in-place conversion\n  - 'teamturbo import --from <source> --to <category>' for remote import")
        }
        (_, None, Some(_)) => {
            anyhow::bail!("--to requires --from. Use 'teamturbo import --from <source> --to <category>'")
        }
        (true, Some(_), None) => {
            anyhow::bail!("--from without --to will convert the source in-place. If that's intended, use: teamturbo import --from <source>")
        }
        (true, None, None) => {
            anyhow::bail!("No paths provided. Usage:\n  - 'teamturbo import <paths...>' for in-place conversion\n  - 'teamturbo import --from <source> --to <category>' for remote import")
        }
    }
}

/// Execute in-place import for multiple paths
async fn execute_in_place_import(paths: Vec<PathBuf>) -> Result<()> {
    println!("{}", style("Mode: In-place conversion").cyan().bold());
    println!("{}", style("Documents will be converted to Docuram format in their current location").dim());
    println!();

    let mut all_files = Vec::new();

    // Collect all markdown files from all paths
    for path in &paths {
        if path.is_file() {
            // Single file
            if !path.extension().map(|e| e == "md" || e == "markdown").unwrap_or(false) {
                println!("{}", style(format!("Skipping non-markdown file: {:?}", path)).yellow());
                continue;
            }
            all_files.push(path.clone());
        } else if path.is_dir() {
            // Directory - scan recursively
            let files = scan_markdown_files(path)?;
            all_files.extend(files);
        }
    }

    if all_files.is_empty() {
        println!("{}", style("No markdown files found").yellow());
        return Ok(());
    }

    println!("{}", style(format!("Found {} markdown file(s)", all_files.len())).bold());
    println!();

    // Process files
    let mut success_count = 0;
    let mut failed_files = Vec::new();

    let pb = ProgressBar::new(all_files.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .expect("Invalid progress bar template")
            .progress_chars("=> ")
    );

    for file_path in &all_files {
        let display_path = file_path.display().to_string();
        pb.set_message(display_path.clone());

        match import_file_in_place(file_path).await {
            Ok(_) => {
                success_count += 1;
            },
            Err(e) => {
                failed_files.push((display_path, e.to_string()));
            }
        }

        pb.inc(1);
    }

    pb.finish_with_message("Done");

    // Report results
    println!();
    if failed_files.is_empty() {
        println!("{}", style(format!("✓ Successfully converted {} document(s)", success_count)).green());
        println!("{}", style("Note: Documents are converted locally. Use 'teamturbo push' to sync them to the server.").cyan());
    } else {
        println!("{}", style(format!("✓ Successfully converted {} document(s)", success_count)).green());
        println!("{}", style(format!("✗ Failed to convert {} document(s):", failed_files.len())).red());
        for (file, error) in failed_files {
            println!("  - {}: {}", file, error);
        }
        println!();
        println!("{}", style("Note: Successfully converted documents are local only. Use 'teamturbo push' to sync them to the server.").cyan());
    }

    Ok(())
}

/// Execute remote import (git clone or external directory to target category)
async fn execute_remote_import(from: String, to: String) -> Result<()> {
    // Determine source type and prepare source
    let (source_path, is_git_repo, is_single_file) = if from.starts_with("http://") || from.starts_with("https://") || from.starts_with("git@") {
        println!("{}", style(format!("Cloning repository: {}", from)).cyan());
        let cloned_dir = clone_git_repo(&from)?;
        (cloned_dir, true, false)
    } else {
        let path = PathBuf::from(&from);
        if !path.exists() {
            anyhow::bail!("Source does not exist: {:?}", path);
        }
        let is_file = path.is_file();
        (path, false, is_file)
    };

    // Get markdown files to import
    let md_files = if is_single_file {
        // Single file import
        if !source_path.extension().map(|e| e == "md" || e == "markdown").unwrap_or(false) {
            anyhow::bail!("File must be a markdown file (.md or .markdown): {:?}", source_path);
        }
        println!("{}", style(format!("Importing single file: {:?}", source_path.file_name().unwrap())).cyan());
        println!();
        vec![source_path.clone()]
    } else {
        // Directory import
        println!("{}", style(format!("Scanning for markdown files in {:?}...", source_path)).cyan());
        println!();

        let files = scan_markdown_files(&source_path)?;

        if files.is_empty() {
            println!("{}", style("No markdown files found").yellow());
            return Ok(());
        }

        println!("{}", style(format!("Found {} markdown file(s)", files.len())).bold());
        println!();
        files
    };

    // Normalize target category path
    let normalized_to = normalize_category_path(&to);

    // Validate category path format
    if normalized_to.is_empty() {
        anyhow::bail!("Category path cannot be empty");
    }
    if normalized_to.contains("//") {
        anyhow::bail!("Invalid category path: contains consecutive slashes");
    }

    // Display target category (will be created during push)
    println!("{}", style(format!("Target category: {}", normalized_to)).cyan());
    println!("{}", style("Category will be created when you push documents").dim());
    println!();

    // Import files
    let mut success_count = 0;
    let mut failed_files = Vec::new();

    let pb = ProgressBar::new(md_files.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .expect("Invalid progress bar template")
            .progress_chars("=> ")
    );

    for md_file in &md_files {
        let relative_path = if is_single_file {
            // For single file, use just the filename
            md_file.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        } else {
            // For directory, use relative path
            md_file.strip_prefix(&source_path)
                .unwrap_or(md_file)
                .to_string_lossy()
                .to_string()
        };

        pb.set_message(format!("{}", relative_path));

        match import_file_remote(md_file, &source_path, &normalized_to, is_single_file).await {
            Ok(_) => {
                success_count += 1;
            },
            Err(e) => {
                failed_files.push((relative_path, e.to_string()));
            }
        }

        pb.inc(1);
    }

    pb.finish_with_message("Done");

    // Report results
    println!();
    if failed_files.is_empty() {
        println!("{}", style(format!("✓ Successfully imported {} document(s) locally", success_count)).green());
        println!("{}", style("Note: Documents are imported locally. Use 'teamturbo push' to sync them to the server.").cyan());
    } else {
        println!("{}", style(format!("✓ Successfully imported {} document(s) locally", success_count)).green());
        println!("{}", style(format!("✗ Failed to import {} document(s):", failed_files.len())).red());
        for (file, error) in failed_files {
            println!("  - {}: {}", file, error);
        }
        println!();
        println!("{}", style("Note: Successfully imported documents are local only. Use 'teamturbo push' to sync them to the server.").cyan());
    }

    // Clean up temporary directory if we cloned a repo
    if is_git_repo {
        println!();
        println!("{}", style("Cleaning up temporary directory...").dim());
        if let Err(e) = fs::remove_dir_all(&source_path) {
            println!("{}", style(format!("Warning: Failed to clean up: {}", e)).yellow());
        }
    }

    Ok(())
}

/// Clone a git repository to a temporary directory
fn clone_git_repo(repo_url: &str) -> Result<PathBuf> {
    use std::process::Command;

    // Create a temporary directory
    let temp_dir = std::env::temp_dir().join(format!("teamturbo-import-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir)?;

    // Clone the repository
    let output = Command::new("git")
        .args(&["clone", "--depth", "1", repo_url, temp_dir.to_str().unwrap()])
        .output()
        .context("Failed to execute git clone. Make sure git is installed.")?;

    if !output.status.success() {
        anyhow::bail!("Git clone failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    println!("{}", style("✓ Repository cloned").green());
    Ok(temp_dir)
}

/// Scan for all markdown files in a directory recursively
fn scan_markdown_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Skip hidden directories and files
        if path.file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with('.'))
            .unwrap_or(false)
        {
            continue;
        }

        // Check if it's a markdown file
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "md" || ext == "markdown" {
                    files.push(path.to_path_buf());
                }
            }
        }
    }

    Ok(files)
}

/// Import a single file in-place (convert at its current location)
async fn import_file_in_place(file_path: &Path) -> Result<()> {
    // Read file content
    let content = fs::read_to_string(file_path)
        .context("Failed to read file")?;

    // Check if already converted
    if extract_uuid_from_frontmatter(&content).is_some() {
        anyhow::bail!("Document already converted: {}", file_path.display());
    }

    // Extract title from filename
    let title = extract_title(file_path, &content)?;

    // Derive category from the file's path relative to docs/
    let full_category = derive_category_from_path(file_path)?;

    // Load docuram.json to check for existing category UUID
    let category_uuid = match DocuramConfig::load() {
        Ok(config) => {
            // Check if the document's category matches the main docuram category
            if full_category == config.docuram.category_path {
                // Use the existing category UUID from docuram.json
                config.docuram.category_uuid.clone()
            } else {
                // Category doesn't match, leave empty for server to assign
                None
            }
        }
        Err(_) => {
            // If docuram.json cannot be loaded, leave empty
            None
        }
    };

    // Create front matter without UUID (server will generate)
    let front_matter = FrontMatter {
        schema: "TEAMTURBO DOCURAM DOCUMENT".to_string(),
        category: full_category,
        title: title.clone(),
        slug: None,
        description: Some("Converted to Docuram format".to_string()),
        doc_type: Some("knowledge".to_string()),
        priority: Some(0),
        is_required: None,
        uuid: None,  // Don't generate UUID, let server handle it
        category_uuid,  // Use existing category UUID if matches, otherwise None
        version: Some(1),
    };

    // Write file with front matter (in-place)
    update_front_matter(file_path, &front_matter, &content)?;

    Ok(())
}

/// Import a single file from remote source to target category
async fn import_file_remote(
    file_path: &Path,
    source_dir: &Path,
    target_category: &str,
    is_single_file: bool,
) -> Result<()> {
    // Read file content
    let content = fs::read_to_string(file_path)
        .context("Failed to read file")?;

    // Extract title from filename
    let title = extract_title(file_path, &content)?;

    // Determine the full category path
    let full_category = if is_single_file {
        // For single file import, use the target category directly
        target_category.to_string()
    } else {
        // For directory import, preserve directory structure as subcategories
        let relative_path = file_path.strip_prefix(source_dir)
            .unwrap_or(file_path);

        let parent_dirs = relative_path.parent()
            .and_then(|p| p.to_str())
            .filter(|s| !s.is_empty());

        if let Some(parent) = parent_dirs {
            format!("{}/{}", target_category, parent.replace("\\", "/"))
        } else {
            target_category.to_string()
        }
    };

    // Determine target file path in docs directory
    let target_dir = PathBuf::from("docs").join(&full_category);
    fs::create_dir_all(&target_dir)?;

    let target_file = target_dir.join(format!("{}.md", sanitize_filename(&title)));

    // Check if target file already exists with valid frontmatter
    if target_file.exists() {
        if let Ok(existing_content) = crate::utils::read_file(&target_file) {
            if extract_uuid_from_frontmatter(&existing_content).is_some() {
                anyhow::bail!("Document already exists at path: {}", target_file.display());
            }
        }
    }

    // Determine source description
    let source_description = if is_single_file {
        format!("Imported from {}", file_path.file_name().unwrap_or_default().to_string_lossy())
    } else {
        let relative_path = file_path.strip_prefix(source_dir).unwrap_or(file_path);
        format!("Imported from {}", relative_path.display())
    };

    // Load docuram.json to check for existing category UUID
    let category_uuid = match DocuramConfig::load() {
        Ok(config) => {
            // Check if the document's category matches the main docuram category
            if full_category == config.docuram.category_path {
                // Use the existing category UUID from docuram.json
                config.docuram.category_uuid.clone()
            } else {
                // Category doesn't match, leave empty for server to assign
                None
            }
        }
        Err(_) => {
            // If docuram.json cannot be loaded, leave empty
            None
        }
    };

    // Create front matter without UUID (server will generate)
    let front_matter = FrontMatter {
        schema: "TEAMTURBO DOCURAM DOCUMENT".to_string(),
        category: full_category.clone(),
        title: title.clone(),
        slug: None,
        description: Some(source_description),
        doc_type: Some("knowledge".to_string()),
        priority: Some(0),
        is_required: None,
        uuid: None,  // Don't generate UUID, let server handle it
        category_uuid,  // Use existing category UUID if matches, otherwise None
        version: Some(1),
    };

    // Write file with front matter
    update_front_matter(&target_file, &front_matter, &content)?;

    // Note: We don't update local state here because the document hasn't been synced to server yet
    // The push command will handle syncing to server and updating state.json

    Ok(())
}

/// Extract title from filename
fn extract_title(file_path: &Path, _content: &str) -> Result<String> {
    // Use filename as title
    let filename = file_path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled");

    Ok(filename.to_string())
}

/// Extract UUID from frontmatter in content
fn extract_uuid_from_frontmatter(content: &str) -> Option<String> {
    // Check if content starts with frontmatter delimiter
    if !content.starts_with("---") {
        return None;
    }

    // Find the end of frontmatter
    let lines: Vec<&str> = content.lines().collect();
    let mut end_index = None;
    for (i, line) in lines.iter().enumerate().skip(1) {
        if line.trim() == "---" {
            end_index = Some(i);
            break;
        }
    }

    if let Some(end) = end_index {
        let frontmatter_text = lines[1..end].join("\n");

        // Parse YAML frontmatter
        if let Ok(frontmatter) = serde_yaml::from_str::<serde_yaml::Value>(&frontmatter_text) {
            // Try to extract uuid from docuram.uuid
            if let Some(docuram) = frontmatter.get("docuram") {
                if let Some(uuid) = docuram.get("uuid") {
                    if let Some(uuid_str) = uuid.as_str() {
                        return Some(uuid_str.to_string());
                    }
                }
            }
        }
    }

    None
}

/// Normalize category path by removing ./docs/ or docs/ prefix and trailing slashes
fn normalize_category_path(path: &str) -> String {
    let trimmed = path.trim();

    let mut result = trimmed;

    // Remove ./docs/ prefix
    if let Some(stripped) = result.strip_prefix("./docs/") {
        result = stripped;
    }
    // Remove docs/ prefix
    else if let Some(stripped) = result.strip_prefix("docs/") {
        result = stripped;
    }
    // Remove ./ prefix
    else if let Some(stripped) = result.strip_prefix("./") {
        result = stripped;
    }

    // Remove trailing slashes
    result.trim_end_matches('/').to_string()
}

/// Sanitize filename to remove invalid characters
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

/// Derive category from file path (for in-place conversion)
/// Expects path to be under docs/ directory
fn derive_category_from_path(file_path: &Path) -> Result<String> {
    // Get absolute path
    let abs_path = file_path.canonicalize()
        .context("Failed to resolve file path")?;

    // Find the docs/ directory in the path
    let mut docs_index = None;
    for (i, component) in abs_path.components().enumerate() {
        if let Some(name) = component.as_os_str().to_str() {
            if name == "docs" {
                docs_index = Some(i);
                break;
            }
        }
    }

    let docs_idx = docs_index.ok_or_else(|| {
        anyhow::anyhow!("File must be under a 'docs/' directory for in-place conversion: {}", file_path.display())
    })?;

    // Get path components after docs/
    let components: Vec<_> = abs_path.components().collect();

    // If file is directly under docs/, use empty category (root)
    // Otherwise, build category path from directory structure
    if docs_idx + 1 >= components.len() - 1 {
        // File is directly under docs/ (e.g., docs/file.md)
        Ok(String::new())
    } else {
        // File is in subdirectory (e.g., docs/category/subcategory/file.md)
        let category_parts: Vec<String> = components[docs_idx + 1..components.len() - 1]
            .iter()
            .filter_map(|c| c.as_os_str().to_str())
            .map(|s| s.to_string())
            .collect();

        Ok(category_parts.join("/"))
    }
}
