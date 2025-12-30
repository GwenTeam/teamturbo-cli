use anyhow::{Context, Result};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use crate::utils::logger;

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    status: i32,
    #[allow(dead_code)]
    error_msg: Option<String>,
    #[allow(dead_code)]
    error_code: Option<i32>,
    config: Option<T>,
}

#[derive(Debug, Deserialize)]
struct DocumentResponse {
    status: i32,
    #[allow(dead_code)]
    error_msg: Option<String>,
    #[allow(dead_code)]
    error_code: Option<i32>,
    document: Option<DocumentContent>,
}

#[derive(Debug, Clone)]
pub struct ApiClient {
    base_url: String,
    token: String,
    client: Client,
}

#[derive(Debug, Deserialize)]
pub struct User {
    pub id: i64,
    pub account: String,
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyResponse {
    pub user: User,
    pub expires_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DocuramConfig {
    pub project: ProjectInfo,
    pub docuram: DocuramInfo,
    #[serde(deserialize_with = "deserialize_documents")]
    pub documents: Vec<DocumentInfo>,
    #[serde(default, deserialize_with = "deserialize_requires")]
    pub requires: Vec<DocumentInfo>,
    pub dependencies: Vec<Dependency>,
    pub category_tree: Option<CategoryTree>,
}

impl DocuramConfig {
    /// Get server URL
    pub fn server_url(&self) -> &str {
        &self.project.url
    }

    /// Get all documents (documents + requires)
    pub fn all_documents(&self) -> impl Iterator<Item = &DocumentInfo> {
        self.documents.iter().chain(self.requires.iter())
    }

    /// Save to docuram/docuram.json
    pub fn save(&self) -> Result<()> {
        use std::path::PathBuf;
        use std::fs;
        use anyhow::Context;

        let path = PathBuf::from("docuram").join("docuram.json");

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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub url: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DocuramInfo {
    pub version: String,  // Keep as String for docuram config version like "1.0.0"
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
    #[serde(default)]
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

    /// Generate local file path based on document type
    /// Documents are organized by type (organic, impl, etc.) directly under docuram/
    /// Preserves the subdirectory structure within each type directory
    /// For example: docuram/organic/subdir/doc.md, docuram/impl/feature/doc.md
    /// Dependencies are placed in docuram/dependencies/ with their category structure
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
            // Dependency documents from other categories go into docuram/dependencies/
            // Preserve the original category structure
            format!("docuram/dependencies/{}/{}", self.category_path, relative_path)
        } else {
            // Main documents are organized by doc_type directly under docuram/
            // Map doc_type to subdirectory (e.g., "knowledge" -> "organic")
            let subdir = match self.doc_type.as_str() {
                "knowledge" => "organic",
                "requirement" => "organic",
                "bug" => "organic",
                "implementation" => "impl",
                "design" => "impl",
                "test" => "impl",
                _ => "organic", // Default to organic for unknown types
            };

            format!("docuram/{}/{}", subdir, relative_path)
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Dependency {
    pub category_id: i64,
    pub category_name: String,
    pub category_path: String,
    pub document_count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CategoryTree {
    pub id: i64,
    pub uuid: Option<String>,
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

#[derive(Debug, Deserialize)]
pub struct CategoryInfo {
    pub id: i64,
    pub uuid: String,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct DocumentContent {
    pub id: i64,
    pub uuid: String,
    pub title: String,
    pub description: Option<String>,
    pub content: Option<String>,
    pub doc_type: String,
    pub status: String,
    pub version: i64,
    pub priority: i64,
    pub is_required: bool,
    pub category: Option<CategoryInfo>,
}

#[derive(Debug, Serialize)]
pub struct DocumentUpdate {
    pub content: String,
    pub change_summary: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DocumentCreate {
    pub category_id: i64,
    pub title: String,
    pub content: String,
    pub description: Option<String>,
    pub doc_type: Option<String>,
    pub priority: Option<i64>,
    pub is_required: Option<bool>,
}

impl ApiClient {
    pub fn new(base_url: String, token: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
            client: Client::new(),
        }
    }

    /// Verify the token is valid
    pub async fn verify(&self) -> Result<VerifyResponse> {
        let url = format!("{}/api/cli/auth/verify", self.base_url);
        logger::http_request("GET", &url);

        let response = self.client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("Failed to verify token")?;

        let status = response.status().as_u16();
        logger::http_response(status, &url);

        match response.status() {
            StatusCode::OK => {
                let data = response.json::<VerifyResponse>()
                    .await
                    .context("Failed to parse verify response")?;
                logger::debug("verify", "Token verified successfully");
                Ok(data)
            }
            StatusCode::UNAUTHORIZED => {
                anyhow::bail!("Token is invalid or expired")
            }
            status => {
                anyhow::bail!("Unexpected status code: {}", status)
            }
        }
    }

    /// Logout and revoke the token
    pub async fn logout(&self) -> Result<()> {
        let url = format!("{}/api/cli/auth/logout", self.base_url);

        let response = self.client
            .delete(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("Failed to logout")?;

        match response.status() {
            StatusCode::OK => Ok(()),
            StatusCode::UNAUTHORIZED => {
                anyhow::bail!("Token is invalid or expired")
            }
            status => {
                anyhow::bail!("Unexpected status code: {}", status)
            }
        }
    }

    /// Get docuram config from URL
    pub async fn get_docuram_config(&self, config_url: &str) -> Result<DocuramConfig> {
        logger::http_request("GET", config_url);
        if logger::is_verbose() {
            println!("[HTTP] Request Headers:");
            println!("  Authorization: Bearer {}...", &self.token[..20.min(self.token.len())]);
        }

        let response = self.client
            .get(config_url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("Failed to fetch docuram config")?;

        let status = response.status().as_u16();
        logger::http_response(status, config_url);

        match response.status() {
            StatusCode::OK => {
                // Get response body as text first for debugging
                let body_text = response.text().await
                    .context("Failed to read response body")?;

                if logger::is_verbose() {
                    println!("[HTTP] Response Body (first 1500 chars):");
                    let preview = if body_text.len() > 1500 {
                        // Use char_indices to find safe UTF-8 boundary
                        let truncate_pos = body_text.char_indices()
                            .nth(1500)
                            .map(|(pos, _)| pos)
                            .unwrap_or(body_text.len());
                        format!("{}...", &body_text[..truncate_pos])
                    } else {
                        body_text.clone()
                    };
                    println!("{}", preview);
                }

                // Parse the API response wrapper
                let api_response: ApiResponse<DocuramConfig> = serde_json::from_str(&body_text)
                    .context("Failed to parse API response")?;

                // Check status and extract config
                if api_response.status != 0 {
                    let error_msg = api_response.error_msg.unwrap_or_else(|| "Unknown error".to_string());
                    anyhow::bail!("API error: {}", error_msg);
                }

                let config = api_response.config
                    .context("Response missing config field")?;

                logger::debug("config", &format!("Downloaded config: {} documents",
                    config.documents.len()));
                Ok(config)
            }
            StatusCode::UNAUTHORIZED => {
                anyhow::bail!("Token is invalid or expired")
            }
            status => {
                anyhow::bail!("Failed to fetch config: {}", status)
            }
        }
    }

    /// Download document content
    pub async fn download_document(&self, uuid: &str) -> Result<DocumentContent> {
        let url = format!("{}/api/docuram/documents/{}", self.base_url, uuid);
        logger::http_request("GET", &url);

        let response = self.client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("Failed to download document")?;

        let status = response.status().as_u16();
        logger::http_response(status, &url);

        match response.status() {
            StatusCode::OK => {
                // Get response body as text first for debugging
                let body_text = response.text().await
                    .context("Failed to read response body")?;

                if logger::is_verbose() {
                    println!("[HTTP] Document Response Body (first 500 chars):");
                    let preview = if body_text.len() > 500 {
                        // Use char_indices to find safe UTF-8 boundary
                        let truncate_pos = body_text.char_indices()
                            .nth(500)
                            .map(|(pos, _)| pos)
                            .unwrap_or(body_text.len());
                        format!("{}...", &body_text[..truncate_pos])
                    } else {
                        body_text.clone()
                    };
                    println!("{}", preview);
                }

                // Parse the API response wrapper
                let api_response: DocumentResponse = serde_json::from_str(&body_text)
                    .context("Failed to parse API response")?;

                // Check status and extract document
                if api_response.status != 0 {
                    let error_msg = api_response.error_msg.unwrap_or_else(|| "Unknown error".to_string());
                    anyhow::bail!("API error: {}", error_msg);
                }

                let doc = api_response.document
                    .context("Response missing document field")?;

                logger::debug("download", &format!("Downloaded document: {} ({})", doc.title, uuid));
                Ok(doc)
            }
            StatusCode::NOT_FOUND => {
                anyhow::bail!("Document not found: {}", uuid)
            }
            StatusCode::UNAUTHORIZED => {
                anyhow::bail!("Token is invalid or expired")
            }
            status => {
                anyhow::bail!("Failed to download document: {}", status)
            }
        }
    }

    /// Upload document content
    pub async fn upload_document(&self, uuid: &str, update: DocumentUpdate) -> Result<DocumentContent> {
        let url = format!("{}/api/docuram/documents/{}", self.base_url, uuid);

        let response = self.client
            .put(&url)
            .bearer_auth(&self.token)
            .json(&update)
            .send()
            .await
            .context("Failed to upload document")?;

        let status = response.status();

        match status {
            StatusCode::OK => {
                let body_text = response.text().await
                    .context("Failed to read response body")?;

                let api_response: DocumentResponse = serde_json::from_str(&body_text)
                    .context("Failed to parse API response")?;

                if api_response.status != 0 {
                    let error_msg = api_response.error_msg.unwrap_or_else(|| "Unknown error".to_string());
                    anyhow::bail!("API error: {}", error_msg);
                }

                let document = api_response.document
                    .context("Response missing document field")?;

                Ok(document)
            }
            StatusCode::NOT_FOUND => {
                anyhow::bail!("Document not found: {}", uuid)
            }
            StatusCode::UNAUTHORIZED => {
                anyhow::bail!("Token is invalid or expired")
            }
            status => {
                anyhow::bail!("Failed to upload document: {}", status)
            }
        }
    }

    /// Create a new document
    pub async fn create_document(&self, doc: DocumentCreate) -> Result<DocumentContent> {
        let url = format!("{}/api/docuram/documents", self.base_url);

        let response = self.client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&doc)
            .send()
            .await
            .context("Failed to create document")?;

        let status = response.status().as_u16();
        logger::http_response(status, &url);

        match response.status() {
            StatusCode::OK | StatusCode::CREATED => {
                let body_text = response.text().await
                    .context("Failed to read response body")?;

                let api_response: DocumentResponse = serde_json::from_str(&body_text)
                    .context("Failed to parse API response")?;

                if api_response.status != 0 {
                    let error_msg = api_response.error_msg.unwrap_or_else(|| "Unknown error".to_string());
                    anyhow::bail!("API error: {}", error_msg);
                }

                let document = api_response.document
                    .context("Response missing document field")?;

                logger::debug("create", &format!("Created document: {} ({})", document.title, document.uuid));
                Ok(document)
            }
            StatusCode::UNAUTHORIZED => {
                anyhow::bail!("Token is invalid or expired")
            }
            StatusCode::BAD_REQUEST => {
                let body = response.text().await.unwrap_or_default();
                anyhow::bail!("Bad request: {}", body)
            }
            status => {
                anyhow::bail!("Failed to create document: {}", status)
            }
        }
    }

    /// Get category ID by path
    pub async fn get_category_by_path(&self, category_path: &str) -> Result<Option<i64>> {
        let url = format!("{}/api/docuram/categories", self.base_url);

        let response = self.client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("Failed to fetch categories")?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let body_text = response.text().await?;
        let api_response: serde_json::Value = serde_json::from_str(&body_text)?;

        // Extract categories array from response
        let categories = api_response.get("categories")
            .and_then(|c| c.as_array())
            .context("No categories in response")?;

        // Recursive function to search for category by path
        fn find_category_id(categories: &[serde_json::Value], path: &str) -> Option<i64> {
            for cat in categories {
                if let Some(cat_path) = cat.get("path").and_then(|p| p.as_str()) {
                    if cat_path == path {
                        return cat.get("id").and_then(|id| id.as_i64());
                    }
                }
                // Search in subcategories
                if let Some(subcats) = cat.get("subcategories").and_then(|s| s.as_array()) {
                    if let Some(id) = find_category_id(subcats, path) {
                        return Some(id);
                    }
                }
            }
            None
        }

        Ok(find_category_id(categories, category_path))
    }

    /// Ensure category exists by path, creating it if necessary
    /// Automatically creates all parent categories in the path
    pub async fn ensure_category_by_path(&self, category_path: &str) -> Result<i64> {
        let url = format!("{}/api/docuram/categories/ensure_by_path", self.base_url);

        logger::http_request("POST", &url);

        let response = self.client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&serde_json::json!({ "path": category_path }))
            .send()
            .await
            .context("Failed to ensure category exists")?;

        let status = response.status().as_u16();
        logger::http_response(status, &url);

        match response.status() {
            StatusCode::OK | StatusCode::CREATED => {
                let body_text = response.text().await
                    .context("Failed to read response body")?;

                let api_response: serde_json::Value = serde_json::from_str(&body_text)
                    .context("Failed to parse API response")?;

                // Check status field
                if let Some(status) = api_response.get("status").and_then(|s| s.as_i64()) {
                    if status != 0 {
                        let error_msg = api_response.get("error_msg")
                            .and_then(|m| m.as_str())
                            .unwrap_or("Unknown error");
                        anyhow::bail!("API error: {}", error_msg);
                    }
                }

                // Extract category ID
                let category_id = api_response
                    .get("category")
                    .and_then(|c| c.get("id"))
                    .and_then(|id| id.as_i64())
                    .context("Response missing category id")?;

                logger::debug("ensure_category", &format!("Ensured category: {} (ID: {})", category_path, category_id));
                Ok(category_id)
            }
            StatusCode::UNAUTHORIZED => {
                anyhow::bail!("Token is invalid or expired")
            }
            StatusCode::BAD_REQUEST => {
                let body = response.text().await.unwrap_or_default();
                anyhow::bail!("Bad request: {}", body)
            }
            status => {
                anyhow::bail!("Failed to ensure category: {}", status)
            }
        }
    }

    /// Get document versions for a category and all its dependencies
    pub async fn get_document_versions(&self, category_uuid: &str) -> Result<Vec<DocumentInfo>> {
        let url = format!("{}/api/docuram/categories/{}/document_versions", self.base_url, category_uuid);

        logger::http_request("GET", &url);

        let response = self.client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("Failed to fetch document versions")?;

        let status = response.status().as_u16();
        logger::http_response(status, &url);

        if !response.status().is_success() {
            anyhow::bail!("Failed to fetch document versions: HTTP {}", status);
        }

        let body_text = response.text().await
            .context("Failed to read response body")?;

        #[derive(Deserialize)]
        struct ApiResponse {
            status: i32,
            #[serde(default)]
            error_msg: String,
            #[serde(default)]
            error_code: i32,
            documents: Vec<DocumentInfo>,
        }

        let api_response: ApiResponse = serde_json::from_str(&body_text)
            .context("Failed to parse document versions response")?;

        if api_response.status != 0 {
            let error_msg = if api_response.error_msg.is_empty() {
                "Unknown error".to_string()
            } else {
                api_response.error_msg
            };
            anyhow::bail!("API error: {}", error_msg);
        }

        Ok(api_response.documents)
    }

    /// Delete a document by UUID
    pub async fn delete_document(&self, uuid: &str) -> Result<()> {
        let url = format!("{}/api/docuram/documents/{}", self.base_url, uuid);

        logger::http_request("DELETE", &url);

        let response = self.client
            .delete(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("Failed to delete document")?;

        let status = response.status().as_u16();
        logger::http_response(status, &url);

        match response.status() {
            StatusCode::OK | StatusCode::NO_CONTENT => {
                logger::debug("delete_document", &format!("Deleted document: {}", uuid));
                Ok(())
            }
            StatusCode::UNAUTHORIZED => {
                anyhow::bail!("Token is invalid or expired")
            }
            StatusCode::NOT_FOUND => {
                anyhow::bail!("Document not found: {}", uuid)
            }
            status => {
                let body = response.text().await.unwrap_or_default();
                anyhow::bail!("Failed to delete document (HTTP {}): {}", status, body)
            }
        }
    }

    /// Delete a category by UUID
    pub async fn delete_category(&self, uuid: &str) -> Result<()> {
        let url = format!("{}/api/docuram/categories/{}", self.base_url, uuid);

        logger::http_request("DELETE", &url);

        let response = self.client
            .delete(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("Failed to delete category")?;

        let status = response.status().as_u16();
        logger::http_response(status, &url);

        match response.status() {
            StatusCode::OK | StatusCode::NO_CONTENT => {
                logger::debug("delete_category", &format!("Deleted category: {}", uuid));
                Ok(())
            }
            StatusCode::UNAUTHORIZED => {
                anyhow::bail!("Token is invalid or expired")
            }
            StatusCode::NOT_FOUND => {
                anyhow::bail!("Category not found: {}", uuid)
            }
            status => {
                let body = response.text().await.unwrap_or_default();
                anyhow::bail!("Failed to delete category (HTTP {}): {}", status, body)
            }
        }
    }

    /// Get category UUID by path
    pub async fn get_category_uuid_by_path(&self, category_path: &str) -> Result<Option<String>> {
        let url = format!("{}/api/docuram/categories", self.base_url);

        let response = self.client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("Failed to fetch categories")?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let body_text = response.text().await?;
        let api_response: serde_json::Value = serde_json::from_str(&body_text)?;

        // Extract categories array from response
        let categories = api_response.get("categories")
            .and_then(|c| c.as_array())
            .context("No categories in response")?;

        // Recursive function to search for category by path
        fn find_category_uuid(categories: &[serde_json::Value], path: &str) -> Option<String> {
            for cat in categories {
                if let Some(cat_path) = cat.get("path").and_then(|p| p.as_str()) {
                    if cat_path == path {
                        return cat.get("uuid").and_then(|id| id.as_str()).map(|s| s.to_string());
                    }
                }
                // Search in subcategories
                if let Some(subcats) = cat.get("subcategories").and_then(|s| s.as_array()) {
                    if let Some(uuid) = find_category_uuid(subcats, path) {
                        return Some(uuid);
                    }
                }
            }
            None
        }

        Ok(find_category_uuid(categories, category_path))
    }

    /// Get documents in a category by path
    pub async fn get_category_documents(&self, category_path: &str) -> Result<Vec<DocumentInfo>> {
        let url = format!("{}/api/docuram/documents", self.base_url);

        let response = self.client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("Failed to fetch documents")?;

        if !response.status().is_success() {
            return Ok(Vec::new());
        }

        let body_text = response.text().await?;
        let api_response: serde_json::Value = serde_json::from_str(&body_text)?;

        // Extract documents array from response
        let documents = api_response.get("documents")
            .and_then(|d| d.as_array())
            .context("No documents in response")?;

        // Filter documents by category path
        let mut category_docs = Vec::new();
        for doc in documents {
            if let Some(doc_cat_path) = doc.get("category_path").and_then(|p| p.as_str()) {
                if doc_cat_path == category_path {
                    // Try to deserialize this document
                    if let Ok(doc_info) = serde_json::from_value::<DocumentInfo>(doc.clone()) {
                        category_docs.push(doc_info);
                    }
                }
            }
        }

        Ok(category_docs)
    }

    /// Send feedback to document authors or category creators
    pub async fn send_feedback(
        &self,
        target_uuids: Vec<String>,
        message: String,
    ) -> Result<FeedbackResponse> {
        let url = format!("{}/api/docuram/feedback", self.base_url);

        // Detect target type (document or category)
        let target_type = self.detect_target_type(&target_uuids[0]).await?;

        let request_body = FeedbackRequest {
            target_type: target_type.to_string(),
            target_uuids,
            message,
        };

        logger::debug("send_feedback", &format!("Sending feedback to {}", url));
        logger::http_request("POST", &url);

        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&request_body)
            .send()
            .await
            .context("Failed to send feedback request")?;

        let status = response.status().as_u16();
        logger::http_response(status, &url);

        match response.status() {
            StatusCode::OK => {
                let feedback_response = response.json::<FeedbackResponse>().await
                    .context("Failed to parse feedback response")?;
                Ok(feedback_response)
            }
            StatusCode::BAD_REQUEST => {
                let error_text = response.text().await.unwrap_or_default();
                anyhow::bail!("Invalid input: {}", error_text)
            }
            StatusCode::UNAUTHORIZED => {
                anyhow::bail!("Authentication required. Run 'teamturbo login' first.")
            }
            StatusCode::NOT_FOUND => {
                anyhow::bail!("Document or category not found. Please verify the UUID is correct.")
            }
            StatusCode::UNPROCESSABLE_ENTITY => {
                let error_text = response.text().await.unwrap_or_default();
                anyhow::bail!("No recipients found: {}", error_text)
            }
            status => {
                let error_text = response.text().await.unwrap_or_default();
                anyhow::bail!("Server error (HTTP {}): {}", status, error_text)
            }
        }
    }

    /// Detect whether UUID is a document or category
    async fn detect_target_type(&self, uuid: &str) -> Result<&'static str> {
        // Try to fetch as document first
        let doc_url = format!("{}/api/docuram/documents/{}", self.base_url, uuid);
        let doc_response = self
            .client
            .get(&doc_url)
            .bearer_auth(&self.token)
            .send()
            .await?;

        if doc_response.status().is_success() {
            return Ok("document");
        }

        // Try as category
        let cat_url = format!("{}/api/docuram/categories/{}", self.base_url, uuid);
        let cat_response = self
            .client
            .get(&cat_url)
            .bearer_auth(&self.token)
            .send()
            .await?;

        if cat_response.status().is_success() {
            return Ok("category");
        }

        anyhow::bail!("UUID not found as document or category: {}", uuid)
    }
}

/// Feedback request structure
#[derive(Debug, Serialize)]
pub struct FeedbackRequest {
    pub target_type: String,
    pub target_uuids: Vec<String>,
    pub message: String,
}

/// Feedback response structure
#[derive(Debug, Deserialize)]
pub struct FeedbackResponse {
    pub success: bool,
    pub recipients: Vec<Recipient>,
    pub message_count: usize,
}

/// Recipient information
#[derive(Debug, Deserialize)]
pub struct Recipient {
    pub user_id: i64,
    pub user_name: String,
    pub email: String,
    pub status: String,
}
