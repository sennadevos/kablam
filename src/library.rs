use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::Context;

/// Sanitise a string for use as a filesystem path component.
/// Replaces / \ : * ? " < > | with _, trims leading/trailing spaces and dots,
/// caps at 200 bytes.
pub fn sanitise(name: &str) -> String {
    const FORBIDDEN: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

    let replaced: String = name
        .chars()
        .map(|c| if FORBIDDEN.contains(&c) { '_' } else { c })
        .collect();

    let trimmed = replaced.trim_matches(|c| c == ' ' || c == '.');

    // Cap at 200 bytes without splitting a multi-byte character.
    if trimmed.len() <= 200 {
        trimmed.to_string()
    } else {
        let mut end = 200;
        while !trimmed.is_char_boundary(end) {
            end -= 1;
        }
        trimmed[..end].to_string()
    }
}

/// Zero-pad a track number to 2 digits (e.g. 3 -> "03").
pub fn pad_track(n: u32) -> String {
    format!("{:02}", n)
}

/// Build the destination path for an identified track.
/// Structure: <root>/<AlbumArtist>/<Year> - <Album>/<Track> - <Title>.<ext>
/// - If year is empty: album dir is just <Album>
/// - If track_number is 0: filename is just <Title>.<ext>
/// - Fallbacks: empty album_artist -> "Unknown Artist", empty album -> "Unknown Album"
pub fn target_path(root: &Path, track: &crate::shazam::TrackResult, ext: &str) -> PathBuf {
    let album_artist = if track.album_artist.is_empty() {
        "Unknown Artist"
    } else {
        &track.album_artist
    };

    let album = if track.album.is_empty() {
        "Unknown Album"
    } else {
        &track.album
    };

    let artist_dir = sanitise(album_artist);

    let album_dir = if track.year.is_empty() {
        sanitise(album)
    } else {
        sanitise(&format!("{} - {}", track.year, album))
    };

    let filename = if track.track_number == 0 {
        format!("{}{}", sanitise(&track.title), ext)
    } else {
        format!(
            "{} - {}{}",
            pad_track(track.track_number),
            sanitise(&track.title),
            ext
        )
    };

    root.join(artist_dir).join(album_dir).join(filename)
}

/// Build destination path for unidentified files: <root>/_Unmatched/<filename>
pub fn unmatched_path(root: &Path, original: &Path) -> PathBuf {
    let filename = original
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("unknown"));
    root.join("_Unmatched").join(filename)
}

/// Move a file from src to dst.
/// - Creates parent directories
/// - Handles filename conflicts by appending _(2), _(3) etc.
/// - If backup: copies src to src + ".bak" first
/// - If dry_run: prints action and returns Ok(())
/// - Falls back to copy+delete on cross-device rename failure
///
/// Returns the actual destination path used (after conflict resolution).
pub fn move_file(
    src: &Path,
    dst: &Path,
    backup: bool,
    dry_run: bool,
    verbose: bool,
) -> anyhow::Result<PathBuf> {
    let resolved_dst = resolve_conflict(dst);

    if dry_run {
        println!(
            "[DRY-RUN] would move: {} -> {}",
            src.display(),
            resolved_dst.display()
        );
        return Ok(resolved_dst);
    }

    // Create parent directories.
    if let Some(parent) = resolved_dst.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    // Optional backup: copy src to <src>.bak
    if backup {
        let mut bak_name = src
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        bak_name.push_str(".bak");
        let bak = src.with_file_name(bak_name);
        fs::copy(src, &bak).with_context(|| {
            format!(
                "Failed to create backup {} -> {}",
                src.display(),
                bak.display()
            )
        })?;
        if verbose {
            println!("[LIBRARY] Backup created: {}", bak.display());
        }
    }

    if verbose {
        println!(
            "[LIBRARY] Moving: {} -> {}",
            src.display(),
            resolved_dst.display()
        );
    }

    // Try rename first; fall back to copy+delete for cross-device moves.
    match fs::rename(src, &resolved_dst) {
        Ok(()) => {}
        Err(e) if is_cross_device(&e) => {
            fs::copy(src, &resolved_dst).with_context(|| {
                format!(
                    "Failed to copy (cross-device) {} -> {}",
                    src.display(),
                    resolved_dst.display()
                )
            })?;
            fs::remove_file(src).with_context(|| {
                format!(
                    "Failed to remove source after cross-device copy: {}",
                    src.display()
                )
            })?;
        }
        Err(e) => {
            return Err(e).with_context(|| {
                format!(
                    "Failed to move {} -> {}",
                    src.display(),
                    resolved_dst.display()
                )
            });
        }
    }

    Ok(resolved_dst)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return a path that does not yet exist by appending _(2), _(3), … before
/// the extension when the desired destination is already occupied.
fn resolve_conflict(dst: &Path) -> PathBuf {
    if !dst.exists() {
        return dst.to_path_buf();
    }

    let stem = dst
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = dst.extension().and_then(|e| e.to_str());
    let parent = dst.parent().unwrap_or_else(|| Path::new("."));

    let mut counter = 2u32;
    loop {
        let fname = match ext {
            Some(e) => format!("_({}).{}", counter, e),
            None => format!("_({})", counter),
        };
        // Prepend the original stem so the result looks like "stem_(2).ext".
        let fname = format!("{}{}", stem, fname);
        let candidate = parent.join(&fname);

        if !candidate.exists() {
            return candidate;
        }
        counter += 1;
    }
}

/// Returns true when the IO error represents a cross-device link error (EXDEV).
fn is_cross_device(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::CrossesDevices || e.raw_os_error() == Some(18)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitise_replaces_forbidden_chars() {
        assert_eq!(sanitise("AC/DC: Best?"), "AC_DC_ Best_");
    }

    #[test]
    fn sanitise_trims_spaces_and_dots() {
        assert_eq!(sanitise("  ..hello.. "), "hello");
    }

    #[test]
    fn sanitise_caps_at_200_bytes() {
        let long = "a".repeat(250);
        assert_eq!(sanitise(&long).len(), 200);
    }

    #[test]
    fn sanitise_respects_char_boundaries() {
        // 'e' with acute = 2 bytes in UTF-8; 100 of them = 200 bytes exactly
        let s = "\u{00e9}".repeat(101); // 202 bytes
        let result = sanitise(&s);
        assert!(result.len() <= 200);
        assert!(result.is_char_boundary(result.len()));
    }

    #[test]
    fn pad_track_single_digit() {
        assert_eq!(pad_track(3), "03");
    }

    #[test]
    fn pad_track_double_digit() {
        assert_eq!(pad_track(12), "12");
    }

    #[test]
    fn target_path_full_metadata() {
        let track = crate::shazam::TrackResult {
            title: "Never Gonna Give You Up".into(),
            artist: "Rick Astley".into(),
            album_artist: "Rick Astley".into(),
            album: "Whenever You Need Somebody".into(),
            year: "1987".into(),
            genre: "Pop".into(),
            track_number: 1,
            cover_art_url: String::new(),
            cover_art_data: Vec::new(),
        };
        let p = target_path(Path::new("/music"), &track, ".mp3");
        assert_eq!(
            p,
            PathBuf::from("/music/Rick Astley/1987 - Whenever You Need Somebody/01 - Never Gonna Give You Up.mp3")
        );
    }

    #[test]
    fn target_path_missing_year_and_track() {
        let track = crate::shazam::TrackResult {
            title: "Mystery".into(),
            artist: "Nobody".into(),
            album_artist: String::new(),
            album: String::new(),
            year: String::new(),
            genre: String::new(),
            track_number: 0,
            cover_art_url: String::new(),
            cover_art_data: Vec::new(),
        };
        let p = target_path(Path::new("/music"), &track, ".flac");
        assert_eq!(
            p,
            PathBuf::from("/music/Unknown Artist/Unknown Album/Mystery.flac")
        );
    }

    #[test]
    fn unmatched_path_uses_original_filename() {
        let p = unmatched_path(Path::new("/music"), Path::new("/tmp/song.mp3"));
        assert_eq!(p, PathBuf::from("/music/_Unmatched/song.mp3"));
    }

    #[test]
    fn resolve_conflict_no_conflict() {
        let tmp = std::env::temp_dir().join("kablam_test_no_conflict.txt");
        // Ensure file doesn't exist
        let _ = fs::remove_file(&tmp);
        assert_eq!(resolve_conflict(&tmp), tmp);
    }

    #[test]
    fn resolve_conflict_appends_counter() {
        let tmp = std::env::temp_dir().join("kablam_test_conflict.txt");
        fs::write(&tmp, "test").unwrap();
        let resolved = resolve_conflict(&tmp);
        assert_eq!(
            resolved,
            std::env::temp_dir().join("kablam_test_conflict_(2).txt")
        );
        let _ = fs::remove_file(&tmp);
    }
}
