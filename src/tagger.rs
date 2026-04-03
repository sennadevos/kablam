use std::path::Path;

use anyhow::Context;
use lofty::config::{ParseOptions, ParsingMode};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::{Accessor, AudioFile, TagExt, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{ItemKey, Tag, TagType};

pub fn write_tags(
    path: &Path,
    track: &crate::shazam::TrackResult,
    dry_run: bool,
    verbose: bool,
) -> anyhow::Result<()> {
    if dry_run {
        println!(
            "[DRY-RUN] would tag: {} -> Title: {}, Artist: {}, Album: {}, Year: {}, Genre: {}",
            path.display(),
            track.title,
            track.artist,
            track.album,
            track.year,
            track.genre,
        );
        return Ok(());
    }

    let parse_opts = ParseOptions::new().parsing_mode(ParsingMode::Relaxed);
    let probe = Probe::open(path)
        .map_err(|e| anyhow::anyhow!("open for tagging {}: {}", path.display(), e))?;

    // Try reading normally; if the file has broken tags, fall back to writing
    // a fresh tag based on the detected file type.
    let read_result = probe.options(parse_opts).read();

    match read_result {
        Ok(mut tagged_file) => {
            let tag = get_or_create_tag(&mut tagged_file);
            write_metadata(tag, track);

            if verbose {
                log_tag(path, track);
            }

            tagged_file
                .save_to_path(path, lofty::config::WriteOptions::default())
                .with_context(|| format!("save tags: {}", path.display()))?;
        }
        Err(e) => {
            if verbose {
                eprintln!("[TAGGER] Read failed ({}), writing fresh tag for {}", e, path.display());
            }

            // Determine tag type from file extension
            let tag_type = tag_type_for_path(path)
                .ok_or_else(|| anyhow::anyhow!("cannot determine tag type for {}", path.display()))?;

            let mut tag = Tag::new(tag_type);
            write_metadata(&mut tag, track);

            if verbose {
                log_tag(path, track);
            }

            tag.save_to_path(path, lofty::config::WriteOptions::default())
                .with_context(|| format!("save fresh tags: {}", path.display()))?;
        }
    }

    Ok(())
}

fn get_or_create_tag(tagged_file: &mut lofty::file::TaggedFile) -> &mut Tag {
    if tagged_file.primary_tag_mut().is_none() {
        let tag_type = tagged_file.primary_tag_type();
        tagged_file.insert_tag(Tag::new(tag_type));
    }
    tagged_file.primary_tag_mut().expect("just inserted tag")
}

fn write_metadata(tag: &mut Tag, track: &crate::shazam::TrackResult) {
    tag.clear();
    tag.set_title(track.title.clone());
    tag.set_artist(track.artist.clone());
    tag.set_album(track.album.clone());
    tag.set_genre(track.genre.clone());

    if !track.album_artist.is_empty() {
        tag.insert_text(ItemKey::AlbumArtist, track.album_artist.clone());
    }
    if let Ok(y) = track.year.parse::<u32>() {
        tag.set_year(y);
    }
    if track.track_number > 0 {
        tag.set_track(track.track_number);
    }
    if !track.cover_art_data.is_empty() {
        let pic = Picture::new_unchecked(
            PictureType::CoverFront,
            Some(MimeType::Jpeg),
            None,
            track.cover_art_data.clone(),
        );
        tag.push_picture(pic);
    }
}

fn tag_type_for_path(path: &Path) -> Option<TagType> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    match ext.as_str() {
        "mp3" => Some(TagType::Id3v2),
        "flac" => Some(TagType::VorbisComments),
        "ogg" | "opus" => Some(TagType::VorbisComments),
        "m4a" | "aac" => Some(TagType::Mp4Ilst),
        "wav" => Some(TagType::Id3v2),
        _ => None,
    }
}

fn log_tag(path: &Path, track: &crate::shazam::TrackResult) {
    println!(
        "[TAGGER] {} -> Title: {}, Artist: {}, Album: {}",
        path.display(),
        track.title,
        track.artist,
        track.album,
    );
}
