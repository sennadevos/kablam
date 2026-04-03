use anyhow::Context;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

const AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "m4a", "aac", "ogg", "opus", "wav"];

fn has_audio_extension(path: &PathBuf) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| AUDIO_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn collect_recursive(dir: &PathBuf, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    let entries = fs::read_dir(dir)
        .with_context(|| format!("Failed to read directory: {}", dir.display()))?;

    for entry in entries {
        let entry = entry
            .with_context(|| format!("Failed to read directory entry in: {}", dir.display()))?;
        let path = entry.path();

        if path.is_dir() {
            let _ = collect_recursive(&path, out);
        } else if path.is_file() && has_audio_extension(&path) {
            out.push(path);
        }
    }

    Ok(())
}

fn collect_shallow(dir: &PathBuf, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    let entries = fs::read_dir(dir)
        .with_context(|| format!("Failed to read directory: {}", dir.display()))?;

    for entry in entries {
        let entry = entry
            .with_context(|| format!("Failed to read directory entry in: {}", dir.display()))?;
        let path = entry.path();

        if path.is_file() && has_audio_extension(&path) {
            out.push(path);
        }
    }

    Ok(())
}

/// Returns a deduplicated, sorted list of absolute audio file paths found
/// under the given input paths. If recursive is false, directories are
/// scanned one level deep only.
pub fn scan(paths: &[PathBuf], recursive: bool) -> anyhow::Result<Vec<PathBuf>> {
    let mut raw: Vec<PathBuf> = Vec::new();
    let mut first_error: Option<anyhow::Error> = None;

    for path in paths {
        if path.is_file() {
            if has_audio_extension(path) {
                raw.push(path.clone());
            }
        } else if path.is_dir() {
            let result = if recursive {
                collect_recursive(path, &mut raw)
            } else {
                collect_shallow(path, &mut raw)
            };

            if let Err(e) = result {
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
        } else {
            let e = anyhow::anyhow!("Path does not exist or is not accessible: {}", path.display());
            if first_error.is_none() {
                first_error = Some(e);
            }
        }
    }

    // Deduplicate by canonical path
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut deduped: Vec<PathBuf> = Vec::new();

    for path in raw {
        match fs::canonicalize(&path) {
            Ok(canonical) => {
                if seen.insert(canonical.clone()) {
                    deduped.push(canonical);
                }
            }
            Err(e) => {
                let wrapped = anyhow::anyhow!(
                    "Failed to canonicalize path {}: {}",
                    path.display(),
                    e
                );
                if first_error.is_none() {
                    first_error = Some(wrapped);
                }
            }
        }
    }

    if deduped.is_empty() {
        if let Some(e) = first_error {
            return Err(e);
        }
    }

    deduped.sort();
    Ok(deduped)
}
