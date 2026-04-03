mod cli;
mod fingerprint;
mod library;
mod processor;
mod progress;
mod scanner;
mod shazam;
mod tagger;
mod watcher;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    match cli.command {
        cli::Command::Process(args) => processor::run(args),
        cli::Command::Watch(args) => watcher::run(args),
    }
}
