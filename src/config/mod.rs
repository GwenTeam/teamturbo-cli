use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::fs;
use crate::auth::AuthConfig;

/// Global CLI configuration
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CliConfig {
    #[serde(flatten)]
    pub auth: std::collections::HashMap<String, AuthConfig>,
}

impl CliConfig {
    /// Get config file path: ~/.teamturbo-cli/config.toml
    pub fn config_path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Failed to get home directory")?;
        Ok(home.join(".teamturbo-cli").join("config.toml"))
    }

    /// Load config from file
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {:?}", path))?;

        toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {:?}", path))
    }

    /// Save config to file
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;

        // Create parent directory if not exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory: {:?}", parent))?;
        }

        let content = toml::to_string_pretty(self)
            .context("Failed to serialize config")?;

        fs::write(&path, content)
            .with_context(|| format!("Failed to write config file: {:?}", path))?;

        Ok(())
    }

    /// Get auth config for a server
    pub fn get_auth(&self, server_url: &str) -> Option<&AuthConfig> {
        self.auth.get(server_url)
    }

    /// Set auth config for a server
    pub fn set_auth(&mut self, server_url: String, auth: AuthConfig) {
        self.auth.insert(server_url, auth);
    }

    /// Remove auth config for a server
    pub fn remove_auth(&mut self, server_url: &str) -> Option<AuthConfig> {
        self.auth.remove(server_url)
    }
}

/// Docuram configuration (docuram.json)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DocuramConfig {
    pub project: ProjectInfo,
    pub docuram: DocuramInfo,
    #[serde(deserialize_with = "deserialize_documents")]
    pub documents: Vec<DocumentInfo>,
    #[serde(default, deserialize_with = "deserialize_requires")]
    pub requires: Vec<DocumentInfo>,
    pub dependencies: Vec<CategoryDependency>,
    pub category_tree: Option<CategoryTree>,

    /// Local documents not yet pushed to server
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub local_documents: Vec<LocalOnlyDocument>,

    /// Public dependencies from docuram.teamturbo.io
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub public_dependencies: Vec<PublicDependency>,
}

/// Local document not yet pushed to server
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocalOnlyDocument {
    /// File path relative to project root
    pub path: String,
    /// Document title (from filename)
    pub title: String,
    /// Local file checksum
    pub checksum: String,
    /// Creation timestamp (ISO 8601 format)
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProjectInfo {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub url: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DocuramInfo {
    pub version: String,
    pub category_id: i64,
    pub category_name: String,
    pub category_uuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_slug: Option<String>,
    pub category_path: String,
    pub task_id: Option<i64>,
    pub task_name: Option<String>,
}

// Custom deserializer to support both old format (required/optional) and new format (array)
fn deserialize_documents<'de, D>(deserializer: D) -> Result<Vec<DocumentInfo>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum DocumentsFormat {
        // New format: direct array
        Array(Vec<DocumentInfo>),
        // Old format: required/optional object
        Object {
            required: Vec<DocumentInfo>,
            optional: Vec<DocumentInfo>,
        },
    }

    match DocumentsFormat::deserialize(deserializer)? {
        DocumentsFormat::Array(docs) => Ok(docs),
        DocumentsFormat::Object { mut required, optional } => {
            // Merge required and optional into single array
            required.extend(optional);
            Ok(required)
        }
    }
}

// Custom deserializer for requires field (defaults to empty array if not present)
fn deserialize_requires<'de, D>(deserializer: D) -> Result<Vec<DocumentInfo>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<Vec<DocumentInfo>>::deserialize(deserializer).map(|opt| opt.unwrap_or_default())
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DocumentInfo {
    // === Server metadata (from server response) ===
    pub id: i64,
    pub uuid: String,
    pub title: String,
    pub category_id: i64,
    pub category_name: String,
    pub category_path: String,
    pub category_uuid: String,
    pub doc_type: String,
    pub version: i64,
    pub path: String,
    pub checksum: String,           // Server checksum
    pub is_required: bool,

    // === Local sync state (optional, set after sync) ===
    /// Local file checksum (used to detect local modifications)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_checksum: Option<String>,

    /// Last sync timestamp (ISO 8601 format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sync: Option<String>,

    /// Mark document as pending deletion (will be deleted from server on next push)
    #[serde(default)]
    pub pending_deletion: bool,
}

impl DocumentInfo {
    /// Generate category remote URL from project URL
    pub fn category_remote_url(&self, project_url: &str) -> String {
        format!("{}/wiki/{}", project_url, self.category_uuid)
    }

    /// Generate document remote URL from project URL
    pub fn remote_url(&self, project_url: &str) -> String {
        format!("{}/wiki/documents/{}", project_url, self.uuid)
    }

    /// Generate local file path based on document type
    /// Documents are organized by type (organic, impl, etc.) directly under docuram/
    /// Preserves the subdirectory structure within each type directory
    /// For example: docuram/organic/subdir/doc.md, docuram/impl/feature/doc.md
    /// Dependencies are placed in dependencies/ (at project root) with their category structure
    pub fn local_path(&self, working_category_path: &str) -> String {
        // Extract the relative path after "docuram/" from the original path
        let path_without_docuram = self.path.strip_prefix("docuram/").unwrap_or(&self.path);

        // Remove the category path prefix to get the relative file path
        // For example: "功能文档/测试新文档结构/subdir/doc.md" -> "subdir/doc.md"
        let relative_path = if path_without_docuram.starts_with(&self.category_path) {
            // Remove category path prefix
            let after_category = path_without_docuram.strip_prefix(&self.category_path)
                .unwrap_or(path_without_docuram);
            // Remove leading slash if present
            after_category.strip_prefix('/').unwrap_or(after_category)
        } else {
            // If path doesn't start with category_path, just use the filename
            path_without_docuram.rsplit('/').next().unwrap_or(path_without_docuram)
        };

        // Check if this document belongs to the working category or its subcategories
        let is_working_category_doc = self.category_path == working_category_path ||
                                       self.category_path.starts_with(&format!("{}/", working_category_path));

        if self.is_required && !is_working_category_doc {
            // Dependency documents from other categories go into dependencies/ (at project root)
            // Preserve the original category structure
            format!("dependencies/{}/{}", self.category_path, relative_path)
        } else {
            // Main documents preserve the subdirectory structure from category_path
            // Extract subdirectory path after working_category_path
            if self.category_path.starts_with(&format!("{}/", working_category_path)) {
                // Has subdirectories under working category
                // E.g., "测试子分类生成/req/001-milestone" with working "测试子分类生成"
                // Should extract "req/001-milestone"
                let subdir_path = self.category_path
                    .strip_prefix(&format!("{}/", working_category_path))
                    .unwrap_or("");

                format!("docuram/{}/{}", subdir_path, relative_path)
            } else {
                // Document is directly in working category, use doc_type mapping
                let subdir = match self.doc_type.as_str() {
                    "knowledge" => "organic",
                    "requirement" => "organic",
                    "bug" => "organic",
                    "implementation" => "impl",
                    "design" => "impl",
                    "test" => "impl",
                    "framework" | "standard" | "spec" | "api" | "troubleshooting" => "manual",
                    _ => "organic", // Default to organic for unknown types
                };

                format!("docuram/{}/{}", subdir, relative_path)
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CategoryDependency {
    pub category_id: i64,
    pub category_name: String,
    pub category_path: String,
    pub document_count: i64,
}

/// Public dependency from docuram.teamturbo.io
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PublicDependency {
    pub category_uuid: String,
    pub category_name: String,
    pub category_path: String,
    pub source_url: String,
    pub document_count: i64,
    #[serde(default)]
    pub documents: Vec<DocumentInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CategoryTree {
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    pub path: String,
    pub description: Option<String>,
    pub position: i64,
    pub parent_id: Option<i64>,
    pub subcategories: Option<Vec<CategoryTree>>,
    pub document_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Installation metadata for CLI upgrades
#[derive(Debug, Serialize, Deserialize)]
pub struct InstallMetadata {
    pub base_url: String,
    pub download_url: String,
    pub install_dir: String,
    pub install_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tt_path: Option<String>,
    pub os: String,
    pub arch: String,
    pub installed_at: String,
}

impl InstallMetadata {
    /// Get install metadata file path: ~/.teamturbo-cli/install.json
    pub fn metadata_path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Failed to get home directory")?;
        Ok(home.join(".teamturbo-cli").join("install.json"))
    }

    /// Load install metadata from file
    pub fn load() -> Result<Self> {
        let path = Self::metadata_path()?;
        if !path.exists() {
            anyhow::bail!("Installation metadata not found. Please reinstall using the installation script.");
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read install metadata: {:?}", path))?;

        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse install metadata: {:?}", path))
    }
}

impl DocuramConfig {
    /// Get docuram.json path (at project root)
    pub fn config_path() -> PathBuf {
        PathBuf::from("docuram.json")
    }

    /// Load from docuram.json
    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        if !path.exists() {
            anyhow::bail!("docuram.json not found. Run 'teamturbo init' first.");
        }

        let content = fs::read_to_string(&path)
            .context("Failed to read docuram.json")?;

        serde_json::from_str(&content)
            .context("Failed to parse docuram.json")
    }

    /// Save to docuram.json
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();

        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize docuram config")?;

        fs::write(&path, content)
            .context("Failed to write docuram.json")?;

        Ok(())
    }

    /// Get server URL
    pub fn server_url(&self) -> &str {
        &self.project.url
    }

    /// Get all documents (documents + requires) as an iterator
    pub fn all_documents(&self) -> impl Iterator<Item = &DocumentInfo> {
        self.documents.iter().chain(self.requires.iter())
    }

    /// Get all documents (documents + requires) as a mutable iterator
    pub fn all_documents_mut(&mut self) -> impl Iterator<Item = &mut DocumentInfo> {
        self.documents.iter_mut().chain(self.requires.iter_mut())
    }

    /// Find document by UUID
    pub fn get_document_by_uuid(&self, uuid: &str) -> Option<&DocumentInfo> {
        self.all_documents().find(|d| d.uuid == uuid)
    }

    /// Find document by UUID (mutable)
    pub fn get_document_by_uuid_mut(&mut self, uuid: &str) -> Option<&mut DocumentInfo> {
        // Can't use all_documents_mut() here due to borrow checker
        self.documents.iter_mut()
            .chain(self.requires.iter_mut())
            .find(|d| d.uuid == uuid)
    }

    /// Find document by path
    pub fn get_document_by_path(&self, path: &str) -> Option<&DocumentInfo> {
        self.all_documents().find(|d| d.path == path)
    }

    /// Find document by path (mutable)
    pub fn get_document_by_path_mut(&mut self, path: &str) -> Option<&mut DocumentInfo> {
        self.documents.iter_mut()
            .chain(self.requires.iter_mut())
            .find(|d| d.path == path)
    }

    /// Find local-only document by path
    pub fn get_local_document_by_path(&self, path: &str) -> Option<&LocalOnlyDocument> {
        self.local_documents.iter().find(|d| d.path == path)
    }

    /// Add a local-only document
    pub fn add_local_document(&mut self, doc: LocalOnlyDocument) {
        // Remove existing if any
        self.local_documents.retain(|d| d.path != doc.path);
        self.local_documents.push(doc);
    }

    /// Remove a local-only document by path
    pub fn remove_local_document(&mut self, path: &str) -> Option<LocalOnlyDocument> {
        let idx = self.local_documents.iter().position(|d| d.path == path)?;
        Some(self.local_documents.remove(idx))
    }

    /// Get documents marked for deletion
    pub fn get_pending_deletions(&self) -> Vec<&DocumentInfo> {
        self.all_documents().filter(|d| d.pending_deletion).collect()
    }

    /// Mark a document as pending deletion
    pub fn mark_for_deletion(&mut self, uuid: &str) -> bool {
        if let Some(doc) = self.get_document_by_uuid_mut(uuid) {
            doc.pending_deletion = true;
            return true;
        }
        false
    }

    /// Remove document by UUID (from documents or requires)
    pub fn remove_document_by_uuid(&mut self, uuid: &str) -> bool {
        let orig_len = self.documents.len();
        self.documents.retain(|d| d.uuid != uuid);
        if self.documents.len() < orig_len {
            return true;
        }

        let orig_len = self.requires.len();
        self.requires.retain(|d| d.uuid != uuid);
        self.requires.len() < orig_len
    }

    /// Load from docuram.json with migration from state.json
    pub fn load_with_migration() -> Result<Self> {
        let mut config = Self::load()?;

        // Check if state.json exists and migrate data
        let state_path = PathBuf::from(".docuram").join("state.json");
        if state_path.exists() {
            if let Ok(content) = fs::read_to_string(&state_path) {
                if let Ok(state) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(docs) = state.get("documents").and_then(|d| d.as_object()) {
                        let mut migrated_count = 0;

                        for (_path, doc_value) in docs {
                            if let (Some(uuid), Some(checksum), Some(last_sync)) = (
                                doc_value.get("uuid").and_then(|v| v.as_str()),
                                doc_value.get("checksum").and_then(|v| v.as_str()),
                                doc_value.get("last_sync").and_then(|v| v.as_str()),
                            ) {
                                // Find matching document in config and update local state
                                if let Some(doc) = config.get_document_by_uuid_mut(uuid) {
                                    if doc.local_checksum.is_none() {
                                        doc.local_checksum = Some(checksum.to_string());
                                        doc.last_sync = Some(last_sync.to_string());
                                        doc.pending_deletion = doc_value.get("pending_deletion")
                                            .and_then(|v| v.as_bool())
                                            .unwrap_or(false);
                                        migrated_count += 1;
                                    }
                                }
                            }
                        }

                        if migrated_count > 0 {
                            // Save migrated config
                            config.save()?;

                            // Remove old state.json
                            let _ = fs::remove_file(&state_path);

                            // Try to remove .docuram directory if empty
                            let docuram_dir = PathBuf::from(".docuram");
                            if let Ok(mut entries) = fs::read_dir(&docuram_dir) {
                                if entries.next().is_none() {
                                    let _ = fs::remove_dir(&docuram_dir);
                                }
                            }

                            println!("Migrated {} document(s) from state.json to docuram.json", migrated_count);
                        }
                    }
                }
            }
        }

        Ok(config)
    }
}
