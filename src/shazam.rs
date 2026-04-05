use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct TrackResult {
    pub title: String,
    pub artist: String,
    pub album_artist: String,
    pub album: String,
    pub year: String,
    pub genre: String,
    pub track_number: u32,
    pub cover_art_url: String,
    pub cover_art_data: Vec<u8>,
    /// Number of Shazam match entries (more = higher confidence)
    pub match_count: usize,
    /// Combined frequency + time skew (lower = more confident match)
    pub match_skew: f64,
}

// --- Serde structs for request body ---

#[derive(Serialize)]
struct ShazamSignature {
    uri: String,
    samplems: u32,
}

#[derive(Serialize)]
struct ShazamRequest {
    timezone: String,
    signature: ShazamSignature,
    timestamp: u128,
    context: serde_json::Value,
    geolocation: serde_json::Value,
}

// --- Serde structs for response ---

#[derive(Deserialize)]
struct ShazamResponse {
    #[serde(default)]
    matches: Vec<serde_json::Value>,
    track: Option<ShazamTrack>,
    retryms: Option<u64>,
}

#[derive(Deserialize)]
struct ShazamTrack {
    title: Option<String>,
    subtitle: Option<String>,
    images: Option<ShazamImages>,
    genres: Option<ShazamGenres>,
    sections: Option<Vec<ShazamSection>>,
}

#[derive(Deserialize)]
struct ShazamImages {
    coverarthq: Option<String>,
    coverart: Option<String>,
}

#[derive(Deserialize)]
struct ShazamGenres {
    primary: Option<String>,
}

#[derive(Deserialize)]
struct ShazamSection {
    #[serde(rename = "type")]
    section_type: Option<String>,
    metadata: Option<Vec<ShazamMetadataEntry>>,
}

#[derive(Deserialize)]
struct ShazamMetadataEntry {
    title: Option<String>,
    text: Option<String>,
}

// --- Helper to extract TrackResult from a ShazamResponse ---

fn extract_track_result(response: &ShazamResponse) -> Option<TrackResult> {
    let track = response.track.as_ref()?;

    let title = track.title.clone().unwrap_or_default();
    let artist = track.subtitle.clone().unwrap_or_default();
    let genre = track
        .genres
        .as_ref()
        .and_then(|g| g.primary.clone())
        .unwrap_or_default();

    let cover_art_url = track
        .images
        .as_ref()
        .and_then(|img| {
            let hq = img.coverarthq.as_deref().unwrap_or("").to_string();
            if !hq.is_empty() {
                Some(hq)
            } else {
                img.coverart.clone()
            }
        })
        .unwrap_or_default();

    let mut album = String::new();
    let mut year = String::new();

    if let Some(sections) = &track.sections {
        for section in sections {
            if section.section_type.as_deref() == Some("SONG") {
                if let Some(metadata) = &section.metadata {
                    for entry in metadata {
                        match entry.title.as_deref() {
                            Some("Album") => {
                                album = entry.text.clone().unwrap_or_default();
                            }
                            Some("Released") => {
                                year = entry.text.clone().unwrap_or_default();
                            }
                            _ => {}
                        }
                    }
                }
                break;
            }
        }
    }

    // Compute confidence from match data
    let match_count = response.matches.len();
    let match_skew = response
        .matches
        .iter()
        .map(|m| {
            let fskew = m["frequencyskew"].as_f64().unwrap_or(1.0).abs();
            let tskew = m["timeskew"].as_f64().unwrap_or(1.0).abs();
            fskew + tskew
        })
        .fold(f64::MAX, f64::min);

    Some(TrackResult {
        title,
        artist: artist.clone(),
        album_artist: artist,
        album,
        year,
        genre,
        track_number: 0,
        cover_art_url,
        cover_art_data: Vec::new(),
        match_count,
        match_skew,
    })
}

// --- Public API ---

/// Identify an audio file by its Shazam signature URI.
/// Returns Ok(None) if the track could not be identified after retries.
pub fn identify(uri: &str, sample_ms: u32, verbose: bool) -> anyhow::Result<Option<TrackResult>> {
    let uuid_a = Uuid::new_v4().to_string().to_uppercase();
    let uuid_b = Uuid::new_v4().to_string().to_uppercase();

    let url = format!(
        "https://amp.shazam.com/discovery/v5/en/GB/iphone/-/tag/{}/{}",
        uuid_a, uuid_b
    );

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis();

    let body = ShazamRequest {
        timezone: "Europe/London".to_string(),
        signature: ShazamSignature {
            uri: uri.to_string(),
            samplems: sample_ms,
        },
        timestamp,
        context: serde_json::json!({}),
        geolocation: serde_json::json!({}),
    };

    let body_json = serde_json::to_string(&body)?;

    const MAX_RETRIES: u32 = 6;

    for attempt in 0..MAX_RETRIES {
        if verbose {
            eprintln!(
                "[shazam] POST {} (attempt {}/{})",
                url,
                attempt + 1,
                MAX_RETRIES
            );
        }

        let response = ureq::post(&url)
            .query("sync", "true")
            .query("webv3", "true")
            .query("sampling", "true")
            .query("connected", "")
            .query("shazamapiversion", "v3")
            .query("sharehub", "true")
            .query("hidelb", "true")
            .set("Content-Type", "application/json")
            .set("X-Shazam-Platform", "IPHONE")
            .set("X-Shazam-AppVersion", "14.1.0")
            .set("Accept", "*/*")
            .set("Accept-Language", "en-US")
            .set(
                "User-Agent",
                "Shazam/3685 CFNetwork/1197 Darwin/20.0.0",
            )
            .send_string(&body_json);

        let response = match response {
            Ok(r) => r,
            Err(ureq::Error::Status(429, _)) => {
                // Rate-limited — back off and retry
                let backoff = Duration::from_secs(2u64.pow(attempt));
                if verbose {
                    eprintln!("[shazam] Rate-limited (429), backing off {}s...", backoff.as_secs());
                }
                sleep(backoff);
                continue;
            }
            Err(ureq::Error::Status(code, r)) => {
                if verbose {
                    eprintln!("[shazam] HTTP error status: {}", code);
                }
                anyhow::bail!(
                    "Shazam API returned HTTP {}: {}",
                    code,
                    r.into_string().unwrap_or_default()
                );
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Shazam request failed: {}", e));
            }
        };

        if verbose {
            eprintln!("[shazam] Response status: {}", response.status());
        }

        let shazam_response: ShazamResponse = response.into_json()?;

        if !shazam_response.matches.is_empty() {
            if verbose {
                eprintln!("[shazam] matches: {}", serde_json::to_string_pretty(&shazam_response.matches).unwrap_or_default());
            }

            // Reject low-confidence matches based on frequency/time skew.
            // Correct matches have skews very close to 0; false positives
            // tend to have skews > 0.01.
            let dominated_by_skew = shazam_response.matches.iter().all(|m| {
                let fskew = m["frequencyskew"].as_f64().unwrap_or(1.0).abs();
                let tskew = m["timeskew"].as_f64().unwrap_or(1.0).abs();
                fskew > 0.01 || tskew > 0.01
            });
            if dominated_by_skew {
                if verbose {
                    eprintln!("[shazam] Match rejected: skew too high (likely false positive)");
                }
                return Ok(None);
            }

            return Ok(extract_track_result(&shazam_response));
        }

        // No match yet; check if we should retry
        let retry_ms = shazam_response.retryms.unwrap_or(0);
        if retry_ms > 0 && attempt + 1 < MAX_RETRIES {
            if verbose {
                eprintln!("[shazam] No match, retrying in {}ms...", retry_ms);
            }
            sleep(Duration::from_millis(retry_ms));
        } else {
            // No retry hint or last attempt
            break;
        }
    }

    Ok(None)
}

/// Download cover art bytes from a URL.
pub fn download_cover_art(url: &str) -> anyhow::Result<Vec<u8>> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| anyhow::anyhow!("Failed to download cover art: {}", e))?;

    let mut bytes: Vec<u8> = Vec::new();
    response.into_reader().read_to_end(&mut bytes)?;

    Ok(bytes)
}
