use anyhow::{Context, Result};
use console::style;
use std::fs;
use std::path::{Path, PathBuf};
use std::collections::HashSet;

use crate::config::DocuramConfig;
use crate::utils::{logger, calculate_checksum};

#[derive(Debug, Clone)]
struct ValidationIssue {
    level: IssueLevel,
    message: String,
}

#[derive(Debug, Clone, PartialEq)]
enum IssueLevel {
    Error,
    Warning,
}

pub async fn execute() -> Result<()> {
    println!("{}", style("Verifying Docuram Project Structure").cyan().bold());
    println!();

    let mut issues: Vec<ValidationIssue> = Vec::new();

    // Check if docuram directory exists
    let docuram_path = Path::new("docuram");
    if !docuram_path.exists() {
        anyhow::bail!("docuram directory not found. Run 'teamturbo init' first.");
    }

    // Load docuram configuration
    let config_path = docuram_path.join("docuram.json");
    if !config_path.exists() {
        anyhow::bail!("docuram.json not found. Run 'teamturbo init' first.");
    }

    let docuram_config = DocuramConfig::load()
        .context("Failed to load docuram.json")?;

    logger::debug("verify", "Loaded docuram.json");

    // 1. Verify category path structure
    println!("{}", style("Checking category path structure...").bold());
    verify_category_path_structure(docuram_path, &docuram_config, &mut issues)?;

    // 2. Verify top-level directory structure
    println!("{}", style("Checking directory structure...").bold());
    verify_directory_structure(docuram_path, &docuram_config, &mut issues)?;

    // 3. Verify req directory contents
    println!("{}", style("Checking req directory...").bold());
    verify_req_directory(docuram_path, &docuram_config, &mut issues)?;

    // 4. Verify dependencies directory (should only contain pulled documents)
    println!("{}", style("Checking dependencies directory...").bold());
    verify_dependencies_directory(docuram_path, &docuram_config, &mut issues)?;

    // 5. Verify document integrity (front matter, checksums)
    println!("{}", style("Checking document integrity...").bold());
    verify_document_integrity(docuram_path, &docuram_config, &mut issues)?;

    // 6. Verify all documents in config exist on disk
    println!("{}", style("Checking document existence...").bold());
    verify_documents_exist(docuram_path, &docuram_config, &mut issues)?;

    println!();

    // Report results
    let errors: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Error).collect();
    let warnings: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Warning).collect();

    let error_count = errors.len();
    let warning_count = warnings.len();

    if !errors.is_empty() {
        println!("{}", style(format!("Found {} error(s):", error_count)).red().bold());
        for issue in &errors {
            println!("  {} {}", style("✗").red(), issue.message);
        }
        println!();
    }

    if !warnings.is_empty() {
        println!("{}", style(format!("Found {} warning(s):", warning_count)).yellow().bold());
        for issue in &warnings {
            println!("  {} {}", style("⚠").yellow(), issue.message);
        }
        println!();
    }

    if issues.is_empty() {
        println!("{}", style("✓ All checks passed! Docuram structure is valid.").green().bold());
        Ok(())
    } else if error_count == 0 {
        println!("{}", style("✓ Verification completed with warnings.").yellow().bold());
        Ok(())
    } else {
        anyhow::bail!("Verification failed with {} error(s)", error_count);
    }
}

fn verify_category_path_structure(
    docuram_path: &Path,
    docuram_config: &DocuramConfig,
    issues: &mut Vec<ValidationIssue>
) -> Result<()> {
    // Get the category path from config
    let category_path = &docuram_config.docuram.category_path;
    let expected_base = docuram_path.join(category_path);

    // Check if the category path directory exists
    if !expected_base.exists() {
        issues.push(ValidationIssue {
            level: IssueLevel::Error,
            message: format!(
                "Category path directory 'docuram/{}' does not exist. Expected based on docuram.category_path.",
                category_path
            ),
        });
        return Ok(());
    }

    // Verify all documents are under the correct category path
    let all_docs: Vec<_> = docuram_config.documents.iter()
        .chain(docuram_config.requires.iter())
        .collect();

    for doc in all_docs {
        let doc_path = Path::new(&doc.path);

        // Document path should start with "docuram/{category_path}/"
        let expected_prefix = format!("docuram/{}/", category_path);

        if !doc.path.starts_with(&expected_prefix) {
            issues.push(ValidationIssue {
                level: IssueLevel::Error,
                message: format!(
                    "Document '{}' is not under the expected category path 'docuram/{}/'",
                    doc.path, category_path
                ),
            });
        }
    }

    Ok(())
}

fn verify_directory_structure(
    docuram_path: &Path,
    docuram_config: &DocuramConfig,
    issues: &mut Vec<ValidationIssue>
) -> Result<()> {
    let category_path = &docuram_config.docuram.category_path;
    let base_path = docuram_path.join(category_path);

    if !base_path.exists() {
        // Already reported in verify_category_path_structure
        return Ok(());
    }

    let allowed_dirs = vec!["dependencies", "impl", "organic", "req"];
    let allowed_files = vec!["README.md"];

    let entries = fs::read_dir(&base_path)
        .with_context(|| format!("Failed to read directory: {}", base_path.display()))?;

    for entry in entries {
        let entry = entry?;
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy().to_string();
        let path = entry.path();

        if path.is_dir() {
            if !allowed_dirs.contains(&name.as_str()) {
                let relative_path = path.strip_prefix(docuram_path)
                    .unwrap_or(&path);
                issues.push(ValidationIssue {
                    level: IssueLevel::Error,
                    message: format!(
                        "Unexpected directory '{}' in {}. Only {:?} are allowed.",
                        name, relative_path.parent().unwrap_or(Path::new("")).display(), allowed_dirs
                    ),
                });
            }
        } else if path.is_file() {
            if !allowed_files.contains(&name.as_str()) {
                let relative_path = path.strip_prefix(docuram_path)
                    .unwrap_or(&path);
                issues.push(ValidationIssue {
                    level: IssueLevel::Error,
                    message: format!(
                        "Unexpected file '{}' in {}. Only {:?} are allowed.",
                        name, relative_path.parent().unwrap_or(Path::new("")).display(), allowed_files
                    ),
                });
            }
        }
    }

    // Check that all required directories exist
    for dir in &allowed_dirs {
        let dir_path = base_path.join(dir);
        if !dir_path.exists() {
            let relative_path = dir_path.strip_prefix(docuram_path)
                .unwrap_or(&dir_path);
            issues.push(ValidationIssue {
                level: IssueLevel::Warning,
                message: format!("Required directory '{}' is missing.", relative_path.display()),
            });
        }
    }

    Ok(())
}

fn verify_req_directory(
    docuram_path: &Path,
    docuram_config: &DocuramConfig,
    issues: &mut Vec<ValidationIssue>
) -> Result<()> {
    let category_path = &docuram_config.docuram.category_path;
    let req_path = docuram_path.join(category_path).join("req");

    if !req_path.exists() {
        // Already warned in directory structure check
        return Ok(());
    }

    let required_files = vec!["README.md", "UPDATED_LOG.md"];

    for file in &required_files {
        let file_path = req_path.join(file);
        if !file_path.exists() {
            let relative_path = file_path.strip_prefix(docuram_path)
                .unwrap_or(&file_path);
            issues.push(ValidationIssue {
                level: IssueLevel::Error,
                message: format!("Required file '{}' is missing.", relative_path.display()),
            });
        }
    }

    Ok(())
}

fn verify_dependencies_directory(
    _docuram_path: &Path,
    docuram_config: &DocuramConfig,
    issues: &mut Vec<ValidationIssue>
) -> Result<()> {
    let working_category_path = &docuram_config.docuram.category_path;

    // Dependencies are now at project root "dependencies/" directory
    let dep_path = Path::new("dependencies");

    if !dep_path.exists() {
        // Dependencies directory doesn't exist, which is fine if there are no requires
        if !docuram_config.requires.is_empty() {
            issues.push(ValidationIssue {
                level: IssueLevel::Warning,
                message: "dependencies/ directory is missing but there are required documents. Run 'teamturbo pull' to download.".to_string(),
            });
        }
        return Ok(());
    }

    // Get all files in dependencies directory recursively
    let dep_files = collect_all_files(dep_path)?;

    // Get all required document LOCAL paths from config
    let required_paths: HashSet<String> = docuram_config.requires.iter()
        .map(|doc| doc.local_path(working_category_path))
        .collect();

    // Check if any file in dependencies is not in the required list
    for file_path in &dep_files {
        let path_str = file_path.to_string_lossy().to_string();

        if !required_paths.contains(&path_str) {
            issues.push(ValidationIssue {
                level: IssueLevel::Error,
                message: format!(
                    "File '{}' in dependencies/ is not a server-pulled dependency. Dependencies should only contain documents pulled from the server.",
                    file_path.display()
                ),
            });
        }
    }

    Ok(())
}

fn verify_document_integrity(
    _docuram_path: &Path,
    docuram_config: &DocuramConfig,
    issues: &mut Vec<ValidationIssue>
) -> Result<()> {
    let working_category_path = &docuram_config.docuram.category_path;

    // Combine all documents (working + dependencies)
    let all_docs: Vec<_> = docuram_config.documents.iter()
        .chain(docuram_config.requires.iter())
        .collect();

    for doc in all_docs {
        // Use local_path() to get the correct local file path
        let local_file_path = doc.local_path(working_category_path);
        let doc_path = Path::new(&local_file_path);

        if !doc_path.exists() {
            // Will be caught in verify_documents_exist
            continue;
        }

        // Read file content
        let content = match fs::read_to_string(&doc_path) {
            Ok(c) => c,
            Err(e) => {
                issues.push(ValidationIssue {
                    level: IssueLevel::Error,
                    message: format!("Failed to read '{}': {}", local_file_path, e),
                });
                continue;
            }
        };

        // Verify checksum
        let calculated_checksum = calculate_checksum(&content);
        if calculated_checksum != doc.checksum {
            issues.push(ValidationIssue {
                level: IssueLevel::Warning,
                message: format!(
                    "Document '{}' has checksum mismatch. File may have been modified.",
                    local_file_path
                ),
            });
        }
    }

    Ok(())
}

fn verify_documents_exist(
    _docuram_path: &Path,
    docuram_config: &DocuramConfig,
    issues: &mut Vec<ValidationIssue>
) -> Result<()> {
    let working_category_path = &docuram_config.docuram.category_path;

    // Check working documents
    for doc in &docuram_config.documents {
        let local_file_path = doc.local_path(working_category_path);
        let doc_path = Path::new(&local_file_path);
        if !doc_path.exists() {
            issues.push(ValidationIssue {
                level: IssueLevel::Error,
                message: format!("Working document '{}' referenced in config but not found on disk.", local_file_path),
            });
        }
    }

    // Check dependency documents
    for doc in &docuram_config.requires {
        let local_file_path = doc.local_path(working_category_path);
        let doc_path = Path::new(&local_file_path);
        if !doc_path.exists() {
            issues.push(ValidationIssue {
                level: IssueLevel::Warning,
                message: format!("Dependency document '{}' referenced in config but not found on disk. Run 'teamturbo pull' to download.", local_file_path),
            });
        }
    }

    Ok(())
}

fn collect_all_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    if !dir.exists() {
        return Ok(files);
    }

    let entries = fs::read_dir(dir)?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            files.push(path);
        } else if path.is_dir() {
            files.extend(collect_all_files(&path)?);
        }
    }

    Ok(files)
}
