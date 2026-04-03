use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::cli::WatchArgs;
use crate::progress;
use crate::scanner;
use crate::processor;

pub fn run(args: WatchArgs) -> Result<()> {
    args.validate()?;

    let inbox = args.inbox_path();
    let library = args.library_path();
    let config = args.config();
    let settle = Duration::from_secs(args.settle);

    fs::create_dir_all(&inbox)
        .with_context(|| format!("create inbox: {}", inbox.display()))?;

    println!("[WATCH] Watching {} -> {}", inbox.display(), library.display());

    // Graceful shutdown on Ctrl-C / SIGTERM
    let running = Arc::new(AtomicBool::new(true));
    {
        let r = running.clone();
        ctrlc::set_handler(move || {
            eprintln!("\n[WATCH] Shutting down...");
            r.store(false, Ordering::Relaxed);
        })?;
    }

    // Process any files already in the inbox
    drain_inbox(&inbox, &config, &library, settle)?;

    // Set up file watcher
    let (tx, rx) = mpsc::channel();
    let mut watcher = create_watcher(tx)?;
    watcher
        .watch(&inbox, RecursiveMode::NonRecursive)
        .with_context(|| format!("watch: {}", inbox.display()))?;

    // Pending files: path -> first-seen time
    let mut pending: HashMap<PathBuf, Instant> = HashMap::new();
    let mut last_api_call = Instant::now() - Duration::from_secs(2);

    while running.load(Ordering::Relaxed) {
        // Collect events (non-blocking, with short timeout)
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Ok(event)) => {
                for path in event.paths {
                    if is_audio_file(&path) && path.is_file() {
                        pending.entry(path).or_insert_with(Instant::now);
                    }
                }
            }
            Ok(Err(e)) => {
                if config.verbose {
                    eprintln!("[WATCH] Watcher error: {}", e);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        // Drain any additional buffered events
        while let Ok(result) = rx.try_recv() {
            if let Ok(event) = result {
                for path in event.paths {
                    if is_audio_file(&path) && path.is_file() {
                        pending.entry(path).or_insert_with(Instant::now);
                    }
                }
            }
        }

        // Process files that have settled
        let ready: Vec<PathBuf> = pending
            .iter()
            .filter(|(_, seen)| seen.elapsed() >= settle)
            .map(|(p, _)| p.clone())
            .collect();

        for path in ready {
            pending.remove(&path);

            // File may have been moved/deleted since we saw it
            if !path.is_file() {
                continue;
            }

            // Final stability check: size hasn't changed
            if !is_stable(&path, settle) {
                // Re-add with fresh timestamp
                pending.insert(path, Instant::now());
                continue;
            }

            // Rate-limit API calls
            let since = last_api_call.elapsed();
            if since < Duration::from_secs(1) {
                sleep(Duration::from_secs(1) - since);
            }

            let status = processor::process_one(&path, &config, &library);
            last_api_call = Instant::now();
            progress::report(&status);
        }
    }

    println!("[WATCH] Stopped.");
    Ok(())
}

/// Process all audio files currently sitting in the inbox.
fn drain_inbox(
    inbox: &PathBuf,
    config: &crate::cli::ProcessConfig,
    library: &std::path::Path,
    settle: Duration,
) -> Result<()> {
    let files = scanner::scan(&[inbox.clone()], false)?;
    if files.is_empty() {
        return Ok(());
    }

    println!("[WATCH] Processing {} existing file(s) in inbox...", files.len());
    for (i, path) in files.iter().enumerate() {
        if i > 0 {
            sleep(Duration::from_secs(1));
        }
        // Wait for stability in case something is still writing
        if !is_stable(path, settle) {
            if config.verbose {
                eprintln!("[WATCH] Skipping unstable file: {}", path.display());
            }
            continue;
        }
        let status = processor::process_one(path, config, library);
        progress::report(&status);
    }
    Ok(())
}

fn create_watcher(
    tx: mpsc::Sender<Result<Event, notify::Error>>,
) -> Result<RecommendedWatcher> {
    let watcher = RecommendedWatcher::new(
        move |res| {
            let _ = tx.send(res);
        },
        notify::Config::default(),
    )
    .context("create file watcher")?;
    Ok(watcher)
}

/// Check that a file's size hasn't changed over `duration`.
fn is_stable(path: &PathBuf, duration: Duration) -> bool {
    let size_a = match fs::metadata(path) {
        Ok(m) => m.len(),
        Err(_) => return false,
    };
    sleep(duration);
    let size_b = match fs::metadata(path) {
        Ok(m) => m.len(),
        Err(_) => return false,
    };
    size_a == size_b && size_a > 0
}

const AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "m4a", "aac", "ogg", "opus", "wav"];

fn is_audio_file(path: &PathBuf) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}
