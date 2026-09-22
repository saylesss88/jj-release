use std::{fs, path::Path};

use jj_release_core::{config, errors::Result};

pub fn init(root: &Path) -> Result<()> {
    let release_toml = root.join("release.toml");
    if release_toml.exists() {
        println!("release.toml already exists. Delete it first to reinitialize.");
        return Ok(());
    }

    let content = config::generate_release_toml(root);
    fs::write(&release_toml, &content)?;
    println!("✓ Written release.toml");

    Ok(())
}
