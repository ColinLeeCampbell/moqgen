use std::path::Path;

/// Returns just filenames — used by subscriber to discover track names
/// and publisher to resolve file paths, without loading file content.
pub fn list_static_dir(dir: &Path) -> anyhow::Result<Vec<String>> {
    let mut entries = collect_filenames(dir)?;
    entries.sort();
    Ok(entries)
}

fn collect_filenames(dir: &Path) -> anyhow::Result<Vec<String>> {
    let read_dir = std::fs::read_dir(dir)
        .map_err(|e| anyhow::anyhow!("failed to read directory '{}': {e}", dir.display()))?;

    let mut names = Vec::new();
    for entry in read_dir {
        let entry = entry.map_err(|e| anyhow::anyhow!("directory entry error: {e}"))?;
        let file_type = entry
            .file_type()
            .map_err(|e| anyhow::anyhow!("file_type error: {e}"))?;
        if file_type.is_dir() {
            continue;
        }
        let os_name = entry.file_name();
        let name = os_name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        names.push(name.into_owned());
    }
    Ok(names)
}
