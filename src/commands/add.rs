use anyhow::{Result, Context};
use console::style;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::config::DocuramConfig;
use crate::utils::{update_front_matter, FrontMatter};

/// Type of organic document to add
#[derive(Debug, Clone, Copy)]
pub enum DocType {
    Req,  // Requirement document
    Bug,  // Bug report document
}

impl DocType {
    fn prefix(&self) -> &str {
        match self {
            DocType::Req => "req",
            DocType::Bug => "bug",
        }
    }

    fn default_header(&self) -> &str {
        match self {
            DocType::Req => "**实现以下需求，并按Docuram规范生成并放置文档**",
            DocType::Bug => "**修正以下错误，并按Docuram规范生成并放置文档**",
        }
    }
}

/// Add a new organic document (req or bug)
pub async fn execute(doc_type: DocType, title: Option<String>) -> Result<()> {
    println!("{}", style("Add Organic Document").cyan().bold());
    println!();

    // Load docuram config to validate we're in a docuram project and get category info
    let docuram_config = DocuramConfig::load()
        .context("Failed to load docuram.json. Run 'teamturbo init' first.")?;

    // Use the organic directory directly under docuram/
    let organic_path = PathBuf::from("docuram/organic");

    // Create organic directory if it doesn't exist
    if !organic_path.exists() {
        fs::create_dir_all(&organic_path)
            .context("Failed to create docuram/organic directory")?;
        println!("{} Created organic directory: {}",
            style("ℹ").blue().bold(),
            style(organic_path.display()).dim()
        );
    }

    // Use the working category path from docuram config and append /organic
    let working_category_path = docuram_config.docuram.category_path.clone();
    let organic_category_path = format!("{}/organic", working_category_path);

    // Get the next available number for this document type
    let next_num = get_next_document_number(&organic_path, doc_type)?;

    // Generate filename
    let filename = generate_filename(doc_type, next_num, title.as_deref());

    // Generate file path
    let file_path = organic_path.join(&filename);

    // Check if file already exists
    if file_path.exists() {
        anyhow::bail!("File already exists: {}", file_path.display());
    }

    // Generate UUID for the new document
    let doc_uuid = Uuid::new_v4().to_string();

    // Create front matter
    let front_matter = FrontMatter {
        schema: "TEAMTURBO DOCURAM DOCUMENT".to_string(),
        category: organic_category_path,
        title: filename.clone(),
        slug: None,
        description: Some("Created by add command".to_string()),
        doc_type: Some("knowledge".to_string()),
        priority: Some(0),
        is_required: None,
        uuid: Some(doc_uuid),
        category_uuid: None, // Will be set when pushed to server
        version: Some(1),
    };

    // Generate document content (without the header, as it's now in front matter context)
    let content = generate_document_content(doc_type, title.as_deref());

    // Write file with front matter
    update_front_matter(&file_path, &front_matter, &content)
        .context(format!("Failed to create file: {}", file_path.display()))?;

    println!("{} {}", 
        style("✓").green().bold(),
        style(format!("Created: {}", file_path.display())).green()
    );
    println!();
    println!("{}", style("Document ready for editing!").dim());

    Ok(())
}


/// Get the next available document number for the given type
fn get_next_document_number(organic_path: &Path, doc_type: DocType) -> Result<usize> {
    let prefix = doc_type.prefix();
    let mut max_num = 0;

    // Read all files in organic directory
    if let Ok(entries) = fs::read_dir(organic_path) {
        for entry in entries.filter_map(|e| e.ok()) {
            if let Some(filename) = entry.file_name().to_str() {
                // Check if filename starts with the prefix (e.g., "req" or "bug")
                if filename.starts_with(prefix) {
                    // Extract number after prefix
                    let after_prefix = &filename[prefix.len()..];
                    // Find the number part (before - or .)
                    let num_str: String = after_prefix
                        .chars()
                        .take_while(|c| c.is_numeric())
                        .collect();

                    if let Ok(num) = num_str.parse::<usize>() {
                        max_num = max_num.max(num);
                    }
                }
            }
        }
    }

    Ok(max_num + 1)
}

/// Generate filename based on document type, number and optional title
fn generate_filename(doc_type: DocType, num: usize, title: Option<&str>) -> String {
    let prefix = doc_type.prefix();
    let num_str = format!("{:03}", num);  // Zero-pad to 3 digits

    match title {
        Some(t) => format!("{}{}-{}.md", prefix, num_str, t),
        None => format!("{}{}.md", prefix, num_str),
    }
}

/// Generate document content
fn generate_document_content(doc_type: DocType, title: Option<&str>) -> String {
    let header = doc_type.default_header();

    match title {
        Some(t) => format!("{}\n\n# {}\n\n", header, t),
        None => format!("{}\n\n", header),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_filename_without_title() {
        assert_eq!(generate_filename(DocType::Req, 1, None), "req001.md");
        assert_eq!(generate_filename(DocType::Bug, 42, None), "bug042.md");
    }

    #[test]
    fn test_generate_filename_with_title() {
        assert_eq!(
            generate_filename(DocType::Req, 1, Some("新功能")),
            "req001-新功能.md"
        );
        assert_eq!(
            generate_filename(DocType::Bug, 5, Some("修复登录问题")),
            "bug005-修复登录问题.md"
        );
    }

    #[test]
    fn test_generate_document_content_without_title() {
        let content = generate_document_content(DocType::Req, None);
        assert!(content.contains("**实现以下需求，并按Docuram规范生成并放置文档**"));
        assert!(!content.contains("# "));
    }

    #[test]
    fn test_generate_document_content_with_title() {
        let content = generate_document_content(DocType::Req, Some("测试标题"));
        assert!(content.contains("**实现以下需求，并按Docuram规范生成并放置文档**"));
        assert!(content.contains("# 测试标题"));
    }
}


