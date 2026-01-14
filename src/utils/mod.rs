pub mod storage;
pub mod logger;

use anyhow::{Result, Context};
use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Calculate SHA-256 checksum of file content
/// Returns checksum in format: "sha256:hexstring"
pub fn calculate_checksum(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

/// Normalize checksum format to ensure it has the "sha256:" prefix
pub fn normalize_checksum(checksum: &str) -> String {
    if checksum.starts_with("sha256:") {
        checksum.to_string()
    } else {
        format!("sha256:{}", checksum)
    }
}

/// Read file content as string
pub fn read_file<P: AsRef<Path>>(path: P) -> Result<String> {
    let content = fs::read_to_string(path.as_ref())?;
    Ok(content)
}

/// Write content to file
pub fn write_file<P: AsRef<Path>>(path: P, content: &str) -> Result<()> {
    // Create parent directories if they don't exist
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path.as_ref(), content)?;
    Ok(())
}

/// Check if file exists and has matching checksum
pub fn verify_checksum<P: AsRef<Path>>(path: P, expected_checksum: &str) -> Result<bool> {
    if !path.as_ref().exists() {
        return Ok(false);
    }

    let content = read_file(path)?;
    let actual_checksum = calculate_checksum(&content);
    Ok(actual_checksum == expected_checksum)
}

/// Format file size in human-readable format
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// YAML front matter wrapper (root level with 'docuram' key)
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FrontMatterWrapper {
    pub docuram: FrontMatter,
}

/// YAML front matter metadata
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FrontMatter {
    pub schema: String,
    pub category: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,  // Optional for backward compatibility, not used
    pub description: Option<String>,
    pub doc_type: Option<String>,
    pub priority: Option<i64>,
    pub is_required: Option<bool>,
    // Additional fields from pull command (optional)
    pub uuid: Option<String>,
    pub category_uuid: Option<String>,
    pub version: Option<i64>,
}

/// Document with front matter and content
#[derive(Debug, Clone)]
pub struct DocumentWithMeta {
    pub front_matter: FrontMatter,
    pub content: String,
    pub file_path: String,
}

/// Extract YAML front matter from markdown content
/// Returns (front_matter, content_without_front_matter)
pub fn extract_front_matter(content: &str) -> Result<Option<(FrontMatter, String)>> {
    let lines: Vec<&str> = content.lines().collect();

    // Check if file starts with ---
    if lines.is_empty() || lines[0].trim() != "---" {
        return Ok(None);
    }

    // Find the closing ---
    let mut end_index = None;
    for (i, line) in lines.iter().enumerate().skip(1) {
        if line.trim() == "---" {
            end_index = Some(i);
            break;
        }
    }

    let end_index = match end_index {
        Some(idx) => idx,
        None => return Ok(None), // No closing ---, not a valid front matter
    };

    // Extract YAML content (between the two ---)
    let yaml_content = lines[1..end_index].join("\n");

    // Parse YAML (try nested format first, then flat format for backward compatibility)
    let front_matter: FrontMatter = if let Ok(wrapper) = serde_yaml::from_str::<FrontMatterWrapper>(&yaml_content) {
        wrapper.docuram
    } else {
        serde_yaml::from_str(&yaml_content)
            .context("Failed to parse YAML front matter")?
    };

    // Validate schema field (support both old and new formats)
    if front_matter.schema != "DOCURAM DOCUMENT" && front_matter.schema != "TEAMTURBO DOCURAM DOCUMENT" {
        return Ok(None); // Not a valid Docuram document
    }

    // Extract remaining content (after the closing ---)
    let content_lines = if end_index + 1 < lines.len() {
        &lines[end_index + 1..]
    } else {
        &[]
    };
    let remaining_content = content_lines.join("\n").trim().to_string();

    Ok(Some((front_matter, remaining_content)))
}

/// Scan a directory for markdown files with or without front matter
pub fn scan_documents_with_meta<P: AsRef<Path>>(dir: P) -> Result<Vec<DocumentWithMeta>> {
    use walkdir::WalkDir;

    let mut documents = Vec::new();

    for entry in WalkDir::new(dir.as_ref())
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Only process .md files
        if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }

        // Read file content
        let content = match read_file(path) {
            Ok(c) => c,
            Err(_) => continue, // Skip files that can't be read
        };

        // Try to extract front matter
        match extract_front_matter(&content) {
            Ok(Some((front_matter, doc_content))) => {
                documents.push(DocumentWithMeta {
                    front_matter,
                    content: doc_content,
                    file_path: path.to_string_lossy().to_string(),
                });
            }
            Ok(None) => {
                // No front matter found, create a default one from filename
                let filename = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                // Use original filename as title (preserving case)
                let title = filename.to_string();
                
                // Create default front matter
                let front_matter = FrontMatter {
                    schema: "TEAMTURBO DOCURAM DOCUMENT".to_string(),
                    category: "".to_string(),
                    title,
                    slug: None,
                    description: None,
                    doc_type: Some("knowledge".to_string()),
                    priority: None,
                    is_required: None,
                    uuid: None,
                    category_uuid: None,
                    version: None,
                };
                
                documents.push(DocumentWithMeta {
                    front_matter,
                    content,
                    file_path: path.to_string_lossy().to_string(),
                });
            }
            Err(_) => {
                // Failed to parse front matter, skip silently
            }
        }
    }

    Ok(documents)
}

/// Update the front matter in a markdown file
pub fn update_front_matter<P: AsRef<Path>>(path: P, front_matter: &FrontMatter, content: &str) -> Result<()> {
    // Create the wrapper for YAML serialization
    let wrapper = FrontMatterWrapper {
        docuram: front_matter.clone(),
    };

    // Serialize to YAML
    let yaml = serde_yaml::to_string(&wrapper)
        .context("Failed to serialize front matter to YAML")?;

    // Build the complete file content
    let mut new_content = String::new();
    new_content.push_str("---\n");
    new_content.push_str(&yaml);
    new_content.push_str("---\n\n");
    new_content.push_str(content);

    // Write to file
    write_file(path, &new_content)?;

    Ok(())
}
