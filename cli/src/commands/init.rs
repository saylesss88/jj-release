use std::{fs, path::Path};

use jj_release_core::{JjBackend, config, errors::Result, jj::ShellBackend};

pub fn init(root: &Path) -> Result<()> {
    let release_toml = root.join("release.toml");
    if release_toml.exists() {
        println!("release.toml already exists. Delete it first to reinitialize.");
        return Ok(());
    }

    let tags = ShellBackend::new(root)
        .and_then(|b| b.list_tags())
        .unwrap_or_default();
    let content = config::generate_release_toml(root, &tags);
    fs::write(&release_toml, &content)?;
    println!("✓ Written release.toml");

    Ok(())
}
