pub mod logger;

use anyhow::Result;
use sha2::{Digest, Sha256};
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
