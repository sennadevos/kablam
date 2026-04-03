# Contributing to Kablam!

Thanks for your interest in contributing!

## Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [FFmpeg](https://ffmpeg.org/) (runtime dependency for audio decoding)

## Building

```bash
git clone https://github.com/sjdevos/kablam.git
cd kablam
cargo build
```

## Running tests

```bash
cargo test
```

Note: Tests cover the pure logic (path sanitisation, library structure, CLI validation). They do not call the Shazam API or require ffmpeg.

## Code style

- Run `cargo fmt` before committing
- Run `cargo clippy` and fix any warnings

## Pull requests

1. Fork the repo and create a branch from `master`
2. Make your changes
3. Add tests if you're adding new logic
4. Ensure `cargo test`, `cargo clippy`, and `cargo fmt --check` all pass
5. Open a PR with a clear description of what you changed and why

## Reporting issues

Open an issue on GitHub. If it's a misidentified song, include:
- The file format (mp3, flac, etc.)
- Whether Shazam's phone app correctly identifies the same audio
- The `--verbose` output
