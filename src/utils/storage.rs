use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Local state tracking file changes
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct LocalState {
    /// Map of document path to local file info (using path as key for better directory structure support)
    pub documents: HashMap<String, LocalDocumentInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocalDocumentInfo {
    pub uuid: String,
    pub path: String,
    pub checksum: String,
    pub version: i64,
    pub last_sync: String,
    #[serde(default = "default_title")]
    pub title: String,
    #[serde(default)]
    pub category_path: String,
    #[serde(default)]
    pub category_uuid: String,
    #[serde(default = "default_doc_type")]
    pub doc_type: String,
    pub description: Option<String>,
    pub priority: Option<i64>,
    #[serde(default)]
    pub is_required: bool,
    /// Mark document as pending deletion (will be deleted from server on next push)
    #[serde(default)]
    pub pending_deletion: bool,
}

/// Default title for backward compatibility
fn default_title() -> String {
    "".to_string()
}

/// Default doc_type for backward compatibility
fn default_doc_type() -> String {
    "knowledge".to_string()
}

impl LocalState {
    /// Get state file path: .docuram/state.json
    pub fn state_path() -> PathBuf {
        PathBuf::from(".docuram").join("state.json")
    }

    /// Load state from file
    pub fn load() -> Result<Self> {
        let path = Self::state_path();
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read state file: {:?}", path))?;

        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse state file: {:?}", path))
    }

    /// Save state to file
    pub fn save(&self) -> Result<()> {
        let path = Self::state_path();

        // Create parent directory if not exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create state directory: {:?}", parent))?;
        }

        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize state")?;

        fs::write(&path, content)
            .with_context(|| format!("Failed to write state file: {:?}", path))?;

        Ok(())
    }

    /// Get document info by UUID (for backward compatibility)
    pub fn get_document_by_uuid(&self, uuid: &str) -> Option<&LocalDocumentInfo> {
        self.documents.values().find(|doc| doc.uuid == uuid)
    }

    /// Get document info by path
    pub fn get_document(&self, path: &str) -> Option<&LocalDocumentInfo> {
        self.documents.get(path)
    }

    /// Update or insert document info using path as key
    pub fn upsert_document(&mut self, info: LocalDocumentInfo) {
        self.documents.insert(info.path.clone(), info);
    }

    /// Remove document info by path
    pub fn remove_document(&mut self, path: &str) -> Option<LocalDocumentInfo> {
        self.documents.remove(path)
    }

    /// Remove document info by UUID (for backward compatibility)
    pub fn remove_document_by_uuid(&mut self, uuid: &str) -> Option<LocalDocumentInfo> {
        // Find the path first
        let path_to_remove = match self.get_document_by_uuid(uuid) {
            Some(doc) => doc.path.clone(),
            None => return None,
        };
        
        self.remove_document(&path_to_remove)
    }

    /// Get all document infos
    pub fn get_all_documents(&self) -> Vec<&LocalDocumentInfo> {
        self.documents.values().collect()
    }

    /// Find document by UUID (for backward compatibility)
    pub fn find_document_by_uuid(&self, uuid: &str) -> Option<&LocalDocumentInfo> {
        self.documents.values().find(|doc| doc.uuid == uuid)
    }
}
