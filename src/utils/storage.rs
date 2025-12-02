use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Local state tracking file changes
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct LocalState {
    /// Map of document uuid to local file info
    pub documents: HashMap<String, LocalDocumentInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocalDocumentInfo {
    pub uuid: String,
    pub path: String,
    pub checksum: String,
    pub version: i64,
    pub last_sync: String,
    /// Mark document as pending deletion (will be deleted from server on next push)
    #[serde(default)]
    pub pending_deletion: bool,
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

    /// Get document info
    pub fn get_document(&self, uuid: &str) -> Option<&LocalDocumentInfo> {
        self.documents.get(uuid)
    }

    /// Update or insert document info
    pub fn upsert_document(&mut self, info: LocalDocumentInfo) {
        self.documents.insert(info.uuid.clone(), info);
    }

    /// Remove document info
    pub fn remove_document(&mut self, uuid: &str) -> Option<LocalDocumentInfo> {
        self.documents.remove(uuid)
    }
}
