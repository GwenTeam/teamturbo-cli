use anyhow::Result;
use console::style;

use crate::commands::{pull, push};

pub async fn execute(force: bool) -> Result<()> {
    println!("{}", style("Sync Documents").cyan().bold());
    println!();

    // First pull updates from server
    println!("{}", style("Step 1/2: Pulling updates from server...").bold());
    println!();

    pull::execute(Vec::new(), force).await?;

    println!();
    println!("{}", style("Step 2/2: Pushing local changes to server...").bold());
    println!();

    // Then push local changes
    push::execute(Vec::new(), Some("Sync: Auto-push after pull".to_string())).await?;

    println!();
    println!("{}", style("✓ Sync completed").green().bold());

    Ok(())
}
