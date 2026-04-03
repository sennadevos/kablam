use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use anyhow::Result;

use crate::cli::{ProcessArgs, ProcessConfig};
use crate::progress::{self, Status};
use crate::{fingerprint, library, scanner, shazam, tagger};

pub fn run(args: ProcessArgs) -> Result<()> {
    args.validate()?;

    let library = args.library_path();
    let config = args.config();
    let files = scanner::scan(&args.paths, args.recursive)?;

    if files.is_empty() {
        anyhow::bail!("no audio files found in the provided paths");
    }

    let (mut identified, mut unmatched, mut errored) = (0u32, 0u32, 0u32);

    for (i, path) in files.iter().enumerate() {
        if i > 0 {
            sleep(Duration::from_secs(1));
        }
        let status = process_one(path, &config, &library);
        match &status {
            Status::Identified { .. } => identified += 1,
            Status::Unmatched { .. } => unmatched += 1,
            Status::Error { .. } => errored += 1,
        }
        progress::report(&status);
    }

    println!("\nDone: {} identified, {} unmatched, {} errors", identified, unmatched, errored);
    Ok(())
}

pub fn process_one(path: &Path, config: &ProcessConfig, library: &Path) -> Status {
    let src = path.display().to_string();

    // 1. Fingerprint
    if config.verbose {
        eprintln!("[FINGERPRINT] {}", path.display());
    }
    let (uri, sample_ms) = match fingerprint::compute(path) {
        Err(e) => return Status::Error { source: src, message: e.to_string() },
        Ok(None) => return handle_unmatched(path, config, library, "audio too short to fingerprint"),
        Ok(Some(v)) => v,
    };

    if config.verbose {
        eprintln!("[FINGERPRINT] {}ms, uri len={}", sample_ms, uri.len());
    }

    // 2. Identify
    let track = match shazam::identify(&uri, sample_ms, config.verbose) {
        Err(e) => return Status::Error { source: src, message: format!("shazam: {}", e) },
        Ok(None) => return handle_unmatched(path, config, library, "not recognised by Shazam"),
        Ok(Some(t)) => t,
    };

    // 3. Download cover art
    let mut track = track;
    if !track.cover_art_url.is_empty() {
        match shazam::download_cover_art(&track.cover_art_url) {
            Ok(data) => track.cover_art_data = data,
            Err(e) if config.verbose => eprintln!("[WARN] cover art: {}", e),
            _ => {}
        }
    }

    // 4. Write tags
    if let Err(e) = tagger::write_tags(path, &track, config.dry_run, config.verbose) {
        return Status::Error { source: src, message: e.to_string() };
    }

    // 5. Move to library
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let dest = library::target_path(library, &track, &ext);

    match library::move_file(path, &dest, config.backup, config.dry_run, config.verbose) {
        Err(e) => Status::Error { source: src, message: e.to_string() },
        Ok(actual_dest) => Status::Identified {
            source: src,
            dest: actual_dest.display().to_string(),
        },
    }
}

fn handle_unmatched(path: &Path, config: &ProcessConfig, library: &Path, reason: &str) -> Status {
    let src = path.display().to_string();
    if config.verbose {
        eprintln!("[UNMATCHED] {}: {}", path.display(), reason);
    }
    match config.unmatched.as_str() {
        "move" => {
            let dest = library::unmatched_path(library, path);
            if let Err(e) = library::move_file(path, &dest, config.backup, config.dry_run, config.verbose) {
                if config.verbose {
                    eprintln!("[WARN] could not move unmatched file: {}", e);
                }
            }
            Status::Unmatched { source: src }
        }
        _ => Status::Unmatched { source: src },
    }
}
