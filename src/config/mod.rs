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
    pub checksum: String,
    pub is_required: bool,
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

    /// Generate local file path based on whether this is a dependency
    /// Required documents (is_required=true) from other categories are placed in dependencies/ subdirectory
    /// Documents from the working category (even if is_required=true) stay in their original path
    /// For example: docuram/working-category/dependencies/dep-category/doc.md
    pub fn local_path(&self, working_category_path: &str) -> String {
        // Check if this document belongs to the working category or its subcategories
        // Use exact match or prefix match with path separator to avoid false positives
        // e.g., "功能文档/测试新文档结构" should match "功能文档/测试新文档结构/requirements"
        // but NOT "功能文档" (which is a parent category)
        let is_working_category_doc = self.category_path == working_category_path ||
                                       self.category_path.starts_with(&format!("{}/", working_category_path));

        if self.is_required && !is_working_category_doc {
            // Dependency documents from other categories go into working_category/dependencies/
            let path_without_docuram = self.path.strip_prefix("docuram/").unwrap_or(&self.path);
            let working_category_normalized = working_category_path.strip_prefix("docuram/").unwrap_or(working_category_path);

            // Place external dependencies under dependencies/
            format!("docuram/{}/dependencies/{}", working_category_normalized, path_without_docuram)
        } else {
            // Main documents and working category documents use path as-is
            self.path.clone()
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CategoryDependency {
    pub category_id: i64,
    pub category_name: String,
    pub category_path: String,
    pub dependency_type: String,
    pub document_count: i64,
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
    /// Get docuram.json path (docuram/docuram.json)
    pub fn config_path() -> PathBuf {
        PathBuf::from("docuram").join("docuram.json")
    }

    /// Load from docuram/docuram.json
    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        if !path.exists() {
            anyhow::bail!("docuram/docuram.json not found. Run 'teamturbo init' first.");
        }

        let content = fs::read_to_string(&path)
            .context("Failed to read docuram/docuram.json")?;

        serde_json::from_str(&content)
            .context("Failed to parse docuram/docuram.json")
    }

    /// Save to docuram/docuram.json
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();

        // Ensure docuram directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {:?}", parent))?;
        }

        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize docuram config")?;

        fs::write(&path, content)
            .context("Failed to write docuram/docuram.json")?;

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
}
