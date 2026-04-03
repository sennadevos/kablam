use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about = "Organise your music library")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Process audio files (identify, tag, and sort)
    Process(ProcessArgs),

    /// Watch an inbox directory and process new files automatically
    Watch(WatchArgs),
}

#[derive(Parser, Debug)]
pub struct ProcessArgs {
    /// One or more audio files or directories
    pub paths: Vec<PathBuf>,

    /// Root of the music library (default: ~/Music)
    #[arg(long)]
    pub library: Option<PathBuf>,

    /// Recurse into subdirectories
    #[arg(short, long)]
    pub recursive: bool,

    /// Print actions without modifying files
    #[arg(long)]
    pub dry_run: bool,

    /// Verbose/debug output
    #[arg(short, long)]
    pub verbose: bool,

    /// Action for unidentified files: move, skip, ignore
    #[arg(long, default_value = "skip")]
    pub unmatched: String,

    /// Back up original file to <file>.bak before modifying
    #[arg(long)]
    pub backup: bool,
}

#[derive(Parser, Debug)]
pub struct WatchArgs {
    /// Directory to watch for new audio files (default: ~/Music/00_Inbox)
    #[arg(long)]
    pub inbox: Option<PathBuf>,

    /// Root of the music library (default: ~/Music)
    #[arg(long)]
    pub library: Option<PathBuf>,

    /// Verbose/debug output
    #[arg(short, long)]
    pub verbose: bool,

    /// Action for unidentified files: move, skip, ignore
    #[arg(long, default_value = "skip")]
    pub unmatched: String,

    /// Back up original file to <file>.bak before modifying
    #[arg(long)]
    pub backup: bool,

    /// Seconds to wait for file size to stabilize (default: 2)
    #[arg(long, default_value = "2")]
    pub settle: u64,
}

/// Shared config extracted from either subcommand for process_one().
pub struct ProcessConfig {
    pub verbose: bool,
    pub dry_run: bool,
    pub unmatched: String,
    pub backup: bool,
}

fn default_library() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join("Music")
}

impl ProcessArgs {
    pub fn library_path(&self) -> PathBuf {
        self.library.clone().unwrap_or_else(default_library)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        validate_unmatched(&self.unmatched)
    }

    pub fn config(&self) -> ProcessConfig {
        ProcessConfig {
            verbose: self.verbose,
            dry_run: self.dry_run,
            unmatched: self.unmatched.clone(),
            backup: self.backup,
        }
    }
}

impl WatchArgs {
    pub fn inbox_path(&self) -> PathBuf {
        self.inbox
            .clone()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("Music/00_Inbox"))
    }

    pub fn library_path(&self) -> PathBuf {
        self.library.clone().unwrap_or_else(default_library)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        validate_unmatched(&self.unmatched)
    }

    pub fn config(&self) -> ProcessConfig {
        ProcessConfig {
            verbose: self.verbose,
            dry_run: false,
            unmatched: self.unmatched.clone(),
            backup: self.backup,
        }
    }
}

fn validate_unmatched(value: &str) -> anyhow::Result<()> {
    match value {
        "move" | "skip" | "ignore" => Ok(()),
        other => anyhow::bail!(
            "Invalid value for --unmatched: {:?}. Must be one of: move, skip, ignore",
            other
        ),
    }
}
