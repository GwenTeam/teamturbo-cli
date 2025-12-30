use anyhow::{Result, Context};
use colored::Colorize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::InstallMetadata;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Execute upgrade command
pub async fn execute(force: bool) -> Result<()> {
    println!("{}", "Checking for updates...".cyan());

    // Load install metadata
    let metadata = InstallMetadata::load()
        .context("Failed to load installation metadata")?;

    // Get current version
    let current_version = VERSION;
    println!("Current version: teamturbo {}", current_version.green());

    // Fetch remote version
    let version_url = format!("{}/teamturbo-cli/version", metadata.base_url);
    println!("Fetching version from: {}", version_url);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let response = client
        .get(&version_url)
        .send()
        .await
        .with_context(|| format!("Failed to fetch version from {}", version_url))?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to fetch version: HTTP {}", response.status());
    }

    let remote_version_text = response.text().await?;
    // Remove "teamturbo " prefix if present
    let remote_version = remote_version_text
        .trim()
        .strip_prefix("teamturbo ")
        .unwrap_or(remote_version_text.trim());

    println!("Latest version: teamturbo {}", remote_version.green());

    // Compare versions
    if remote_version == current_version {
        println!("{}", "You are already using the latest version!".green());
        return Ok(());
    }

    // Parse versions for comparison
    let current_parts: Vec<u32> = current_version
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();
    let remote_parts: Vec<u32> = remote_version
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();

    let is_newer = remote_parts > current_parts;

    if !is_newer {
        println!(
            "{}",
            format!(
                "Local version ({}) is newer than remote version ({})",
                current_version, remote_version
            )
            .yellow()
        );
        return Ok(());
    }

    println!(
        "{}",
        format!(
            "New version available: {} -> {}",
            current_version, remote_version
        )
        .green()
    );

    // Ask for confirmation unless force flag is set
    if !force {
        println!("\n{}", "Do you want to upgrade? (y/N): ".cyan());
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();

        if input != "y" && input != "yes" {
            println!("{}", "Upgrade cancelled.".yellow());
            return Ok(());
        }
    } else {
        println!("{}", "\nForce upgrade mode: skipping confirmation.".yellow());
    }

    println!("{}", "Downloading new version...".cyan());

    // Download new version
    let response = client
        .get(&metadata.download_url)
        .send()
        .await
        .with_context(|| format!("Failed to download from {}", metadata.download_url))?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to download: HTTP {}", response.status());
    }

    let bytes = response.bytes().await?;

    // Create temp file
    let temp_dir = std::env::temp_dir();
    let temp_file = if metadata.os == "Windows" {
        temp_dir.join("teamturbo_upgrade.zip")
    } else {
        temp_dir.join("teamturbo_upgrade.gz")
    };

    fs::write(&temp_file, &bytes)
        .with_context(|| format!("Failed to write temp file: {:?}", temp_file))?;

    println!("{}", "Extracting files...".cyan());

    // Extract and install based on OS
    if metadata.os == "Windows" {
        install_windows(&temp_file, &metadata)?;
    } else {
        install_unix(&temp_file, &metadata)?;
    }

    // Clean up temp file
    let _ = fs::remove_file(&temp_file);

    println!("{}", "\nUpgrade completed successfully!".green());
    println!(
        "{}",
        format!("teamturbo {} -> {}", current_version, remote_version).green()
    );
    println!("\nRun 'teamturbo --version' to verify the update.");

    Ok(())
}

fn install_windows(zip_path: &Path, metadata: &InstallMetadata) -> Result<()> {
    use std::io::Read;
    use zip::ZipArchive;

    let file = fs::File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)?;

    // Extract the binary
    let mut entry = archive.by_index(0)?;
    let mut buffer = Vec::new();
    entry.read_to_end(&mut buffer)?;

    // Get install paths
    let install_path = Path::new(&metadata.install_path);
    let tt_path_buf = metadata
        .tt_path
        .as_ref()
        .map(|p| PathBuf::from(p))
        .unwrap_or_else(|| PathBuf::from(&metadata.install_dir).join("tt.exe"));
    let tt_path = tt_path_buf.as_path();

    // Write to temporary files first (to avoid file in use errors)
    let temp_teamturbo_path = install_path.with_extension("new.exe");
    let temp_tt_path = tt_path.with_extension("new.exe");

    fs::write(&temp_teamturbo_path, &buffer)
        .with_context(|| format!("Failed to write to {:?}", temp_teamturbo_path))?;

    // Copy to tt temporary file
    fs::copy(&temp_teamturbo_path, &temp_tt_path)
        .with_context(|| format!("Failed to copy to {:?}", temp_tt_path))?;

    // Try to rename/replace the files
    // On Windows, if the file is in use, we may need to wait a moment
    let max_attempts = 3;
    for attempt in 1..=max_attempts {
        match fs::rename(&temp_teamturbo_path, install_path) {
            Ok(_) => break,
            Err(e) if attempt < max_attempts => {
                println!("Waiting for file to be available (attempt {}/{})...", attempt, max_attempts);
                std::thread::sleep(std::time::Duration::from_millis(500));
                if attempt == max_attempts - 1 {
                    return Err(e).with_context(|| format!("Failed to replace {:?}. Please close all terminal windows running teamturbo and try again.", install_path));
                }
            }
            Err(e) => return Err(e).with_context(|| format!("Failed to replace {:?}", install_path)),
        }
    }

    // Replace tt.exe
    fs::rename(&temp_tt_path, tt_path)
        .with_context(|| format!("Failed to replace {:?}", tt_path))?;

    println!(
        "Updated: {} and {}",
        install_path.display(),
        tt_path.display()
    );

    Ok(())
}

fn install_unix(gz_path: &Path, metadata: &InstallMetadata) -> Result<()> {
    use flate2::read::GzDecoder;
    use std::io::Read;

    let file = fs::File::open(gz_path)?;
    let mut decoder = GzDecoder::new(file);
    let mut buffer = Vec::new();
    decoder.read_to_end(&mut buffer)?;

    let install_path = Path::new(&metadata.install_path);

    // Write to a temporary file first (to avoid "Text file busy" error)
    let temp_new_path = install_path.with_extension("new");
    fs::write(&temp_new_path, &buffer)
        .with_context(|| format!("Failed to write to {:?}", temp_new_path))?;

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&temp_new_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&temp_new_path, perms)?;
    }

    // Use rename/move to replace the running binary (this works even if file is in use)
    fs::rename(&temp_new_path, install_path)
        .with_context(|| format!("Failed to replace binary at {:?}", install_path))?;

    println!("Updated: {}", install_path.display());

    // Note: tt is a symlink, no need to update it

    Ok(())
}
