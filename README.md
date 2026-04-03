# Kablam!

[![CI](https://github.com/sennadevos/kablam/actions/workflows/ci.yml/badge.svg)](https://github.com/sennadevos/kablam/actions/workflows/ci.yml)
[![Release](https://github.com/sennadevos/kablam/actions/workflows/release.yml/badge.svg)](https://github.com/sennadevos/kablam/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**Shazam-powered CLI that identifies, tags, and sorts your music library — no filenames needed.**

Got a folder full of `track_01_final_FINAL(2).mp3` and `Unknown Artist - Unknown Album.opus`? Kablam! listens to the actual audio, asks Shazam what it is, slaps on proper metadata + cover art, and files it away neatly. Your music hoarding days of chaos are over.

## Features

- Identifies songs using Shazam's audio fingerprinting algorithm (DecibelX)
- Ignores filenames and existing metadata entirely — uses only the audio content
- Writes ID3/Vorbis/MP4 tags: title, artist, album, album artist, year, genre, track number
- Embeds high-quality cover art
- Sorts files into a clean library structure
- **Watch mode**: runs as a daemon, automatically processing new files dropped into an inbox
- Handles malformed tags from yt-dlp and other downloaders
- Confidence filtering to reject false-positive Shazam matches
- Rate limiting with exponential backoff
- Cross-device file moves with automatic fallback to copy+delete
- Conflict resolution (appends `_(2)`, `_(3)`, etc.)

## Prerequisites

- [Rust](https://rustup.rs/) (for building)
- [FFmpeg](https://ffmpeg.org/) (runtime dependency — used to decode audio)

## Installation

### Quick install (Linux / macOS)

```bash
curl -fsSL https://raw.githubusercontent.com/sennadevos/kablam/main/install.sh | sh
```

This downloads the latest pre-built binary for your platform to `~/.local/bin`.

### Pre-built binaries

Grab the latest release from [GitHub Releases](https://github.com/sennadevos/kablam/releases) and extract it:

```bash
tar xzf kablam-x86_64-unknown-linux-gnu.tar.gz
sudo mv kablam /usr/local/bin/
```

### From source

```bash
cargo install --git https://github.com/sennadevos/kablam
```

Or clone and build:

```bash
git clone https://github.com/sennadevos/kablam.git
cd kablam
cargo build --release
# Binary is at ./target/release/kablam
```

## Usage

### Process files (batch mode)

```bash
# Process specific files
kablam process song1.mp3 song2.flac

# Process a directory
kablam process ~/Downloads/music/

# Process recursively
kablam process ~/Downloads/music/ -r

# Specify a library destination
kablam process ~/Music/00_Inbox -r --library ~/Music/Library

# Dry run — see what would happen without touching files
kablam process ~/Music/00_Inbox --dry-run --verbose
```

### Watch mode (daemon)

```bash
# Watch an inbox and auto-sort new files
kablam watch --inbox ~/Music/00_Inbox --library ~/Music/Library

# With verbose output
kablam watch --inbox ~/Music/00_Inbox --library ~/Music/Library --verbose
```

Files dropped into the inbox are automatically fingerprinted, identified, tagged, and moved to the library. Kablam! waits for files to finish downloading before processing them.

### Library structure

Files are sorted into:

```
~/Music/Library/
  Artist Name/
    2024 - Album Name/
      01 - Song Title.mp3
```

Unmatched files (songs Shazam can't identify) are left untouched by default.

## CLI Reference

### `kablam process`

| Flag | Description |
|------|-------------|
| `<paths...>` | One or more audio files or directories |
| `--library <path>` | Root of the music library (default: `~/Music`) |
| `-r`, `--recursive` | Recurse into subdirectories |
| `--dry-run` | Print actions without modifying files |
| `-v`, `--verbose` | Verbose/debug output |
| `--unmatched <action>` | `skip` (default), `move`, or `ignore` unidentified files |
| `--backup` | Create `.bak` backup before modifying files |

### `kablam watch`

| Flag | Description |
|------|-------------|
| `--inbox <path>` | Directory to watch (default: `~/Music/00_Inbox`) |
| `--library <path>` | Root of the music library (default: `~/Music`) |
| `-v`, `--verbose` | Verbose/debug output |
| `--unmatched <action>` | `skip` (default), `move`, or `ignore` unidentified files |
| `--backup` | Create `.bak` backup before modifying files |
| `--settle <secs>` | Seconds to wait for file size to stabilize (default: `2`) |

### Supported formats

mp3, flac, m4a, aac, ogg, opus, wav

## Running as a systemd service

```bash
# Copy the service file
cp contrib/kablam.service ~/.config/systemd/user/

# Enable and start
systemctl --user enable --now kablam

# Check status
systemctl --user status kablam

# View logs
journalctl --user -u kablam -f
```

Edit the service file to customise `--inbox` and `--library` paths.

## Known limitations

- **Rate limiting**: Shazam will throttle you if you process too many files too quickly. Kablam! has a 1-second delay between requests and retries with exponential backoff on 429s, but very large batches may still hit limits.
- **Ambient/instrumental tracks**: Very ambient or short interlude tracks may not be in Shazam's database, or may match incorrectly. Kablam! filters out low-confidence matches, but some edge cases may slip through.
- **Cover art**: Always embedded as JPEG. If Shazam doesn't have cover art for a track, none is embedded.
- **Track numbers**: Shazam rarely provides track numbers, so most files won't have them.

## License

[MIT](LICENSE)
