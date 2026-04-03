use anyhow::{anyhow, Context};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use crc32fast::Hasher as Crc32Hasher;
use rustfft::{num_complex::Complex, FftPlanner};
use std::io::Write;
use std::path::Path;
use std::process::Command;

// ── Constants matching SongRec ────────────────────────────────────────────────

const SAMPLE_RATE: u32 = 16_000;
const FFT_SIZE: usize = 2048;
const HOP_SIZE: usize = 128;
const NUM_BINS: usize = FFT_SIZE / 2 + 1; // 1025
const RING: usize = 256;
const MAX_SECS: f32 = 12.0;
const DATA_URI_PREFIX: &str = "data:audio/vnd.shazam.sig;base64,";

// ── Frequency band ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum FrequencyBand {
    B250_520 = 0,
    B520_1450 = 1,
    B1450_3500 = 2,
    B3500_5500 = 3,
}

impl FrequencyBand {
    fn wire_tag(self) -> u32 {
        0x60030040 + self as u32
    }
}

fn band_for_hz(hz: f32) -> Option<FrequencyBand> {
    match hz as i32 {
        250..=519 => Some(FrequencyBand::B250_520),
        520..=1449 => Some(FrequencyBand::B520_1450),
        1450..=3499 => Some(FrequencyBand::B1450_3500),
        3500..=5500 => Some(FrequencyBand::B3500_5500),
        _ => None,
    }
}

// ── Peak ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct FrequencyPeak {
    fft_pass_number: u32,
    peak_magnitude: u16,
    corrected_peak_frequency_bin: u16,
}

// ── SignatureGenerator (ported from SongRec algorithm.rs) ─────────────────────

struct SignatureGenerator {
    ring_buffer: Vec<f32>,
    ring_buffer_index: usize,
    reordered: Vec<f32>,

    fft_outputs: Vec<Vec<f32>>,
    fft_outputs_index: usize,

    spread_fft_outputs: Vec<Vec<f32>>,
    spread_fft_outputs_index: usize,

    num_spread_ffts_done: u32,
    hanning: Vec<f32>,
    peaks: Vec<(FrequencyBand, FrequencyPeak)>,
    number_samples: u32,
}

impl SignatureGenerator {
    fn new(num_samples: u32) -> Self {
        let hanning = (0..FFT_SIZE)
            .map(|i| {
                0.5 * (1.0
                    - (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos())
            })
            .collect();

        SignatureGenerator {
            ring_buffer: vec![0.0; FFT_SIZE],
            ring_buffer_index: 0,
            reordered: vec![0.0; FFT_SIZE],
            fft_outputs: vec![vec![0.0; NUM_BINS]; RING],
            fft_outputs_index: 0,
            spread_fft_outputs: vec![vec![0.0; NUM_BINS]; RING],
            spread_fft_outputs_index: 0,
            num_spread_ffts_done: 0,
            hanning,
            peaks: Vec::new(),
            number_samples: num_samples,
        }
    }

    fn do_fft(&mut self, chunk: &[f32], fft: &dyn rustfft::Fft<f32>) {
        // Write i16-scale samples into ring buffer
        let idx = self.ring_buffer_index;
        for (i, &s) in chunk.iter().enumerate() {
            self.ring_buffer[(idx + i) & (FFT_SIZE - 1)] = s * 32768.0;
        }
        self.ring_buffer_index = (self.ring_buffer_index + HOP_SIZE) & (FFT_SIZE - 1);

        // Reorder (oldest first) + Hanning window
        let ri = self.ring_buffer_index;
        for i in 0..FFT_SIZE {
            self.reordered[i] = self.ring_buffer[(ri + i) & (FFT_SIZE - 1)] * self.hanning[i];
        }

        // FFT
        let mut buf: Vec<Complex<f32>> = self
            .reordered
            .iter()
            .map(|&r| Complex { re: r, im: 0.0 })
            .collect();
        fft.process(&mut buf);

        // Power spectrum / 131072, clamped (SongRec normalization)
        let out = &mut self.fft_outputs[self.fft_outputs_index];
        for i in 0..NUM_BINS {
            out[i] = ((buf[i].re * buf[i].re + buf[i].im * buf[i].im) / 131072.0).max(1e-10);
        }
        self.fft_outputs_index = (self.fft_outputs_index + 1) & (RING - 1);
    }

    fn do_peak_spreading(&mut self) {
        // Source: the FFT output just written
        let src_idx = (self.fft_outputs_index + RING - 1) & (RING - 1);
        let src = self.fft_outputs[src_idx].clone();

        // Frequency-domain spreading: forward lookahead of 2 bins (SongRec)
        let dest_idx = self.spread_fft_outputs_index;
        let spread = &mut self.spread_fft_outputs[dest_idx];
        spread.copy_from_slice(&src);
        for i in 0..=1022 {
            let v = spread[i].max(spread[i + 1]).max(spread[i + 2]);
            spread[i] = v;
        }

        // Time-domain back-propagation into earlier spread entries
        let spread_copy = self.spread_fft_outputs[dest_idx].clone();
        for &off in &[1i32, 3, 6] {
            let prev_idx = (dest_idx as i32 - off).rem_euclid(RING as i32) as usize;
            let prev = &mut self.spread_fft_outputs[prev_idx];
            for i in 0..NUM_BINS {
                if spread_copy[i] > prev[i] {
                    prev[i] = spread_copy[i];
                }
            }
        }

        self.spread_fft_outputs_index = (self.spread_fft_outputs_index + 1) & (RING - 1);
        self.num_spread_ffts_done += 1;
    }

    fn do_peak_recognition(&mut self) {
        let fft_minus_46 = {
            let idx = (self.fft_outputs_index as i32 - 46).rem_euclid(RING as i32) as usize;
            self.fft_outputs[idx].clone()
        };
        let fft_minus_49 = {
            let idx =
                (self.spread_fft_outputs_index as i32 - 49).rem_euclid(RING as i32) as usize;
            self.spread_fft_outputs[idx].clone()
        };

        for bin in 10..=1014usize {
            let v = fft_minus_46[bin];
            if v < 1.0 / 64.0 {
                continue;
            }
            if v < fft_minus_49[bin - 1] {
                continue;
            }

            // Frequency neighbors
            let mut max_freq: f32 = 0.0;
            for &off in &[-10i32, -7, -4, -3, 1, 2, 5, 8] {
                let nb = (bin as i32 + off) as usize;
                max_freq = max_freq.max(fft_minus_49[nb]);
            }
            if v <= max_freq {
                continue;
            }

            // Time neighbors
            let mut max_time = max_freq;
            for &off in &[
                -53i32, -45, 165, 172, 179, 186, 193, 200, 214, 221, 228, 235, 242, 249,
            ] {
                let idx = (self.spread_fft_outputs_index as i32 + off)
                    .rem_euclid(RING as i32) as usize;
                max_time = max_time.max(self.spread_fft_outputs[idx][bin - 1]);
            }
            if v <= max_time {
                continue;
            }

            // ── Peak found ────────────────────────────────────────────────────
            let fft_pass_number = self.num_spread_ffts_done - 46;

            let mag = |x: f32| x.ln().max(1.0_f32 / 64.0) * 1477.3 + 6144.0;
            let pm = mag(v);
            let pm_before = mag(fft_minus_46[bin - 1]);
            let pm_after = mag(fft_minus_46[bin + 1]);

            let var1 = pm * 2.0 - pm_before - pm_after;
            if var1 < 0.0 {
                continue;
            }
            let var2 = (pm_after - pm_before) * 32.0 / var1;
            let corrected_peak_frequency_bin = ((bin as i32 * 64) + var2 as i32) as u16;

            let freq_hz =
                corrected_peak_frequency_bin as f32 * (16000.0 / 2.0 / 1024.0 / 64.0);
            let band = match band_for_hz(freq_hz) {
                Some(b) => b,
                None => continue,
            };

            self.peaks.push((
                band,
                FrequencyPeak {
                    fft_pass_number,
                    peak_magnitude: pm as u16,
                    corrected_peak_frequency_bin,
                },
            ));
        }
    }

    fn process_and_encode(mut self, samples: &[f32], fft: &dyn rustfft::Fft<f32>) -> Option<(String, u32)> {
        let max_samples = (MAX_SECS as usize) * SAMPLE_RATE as usize;
        let samples = if samples.len() > max_samples {
            let mid = samples.len() / 2;
            let half = max_samples / 2;
            &samples[mid - half..mid + half]
        } else {
            samples
        };

        // number_samples must reflect the slice actually fingerprinted, not the
        // full file length — Shazam uses this to interpret peak timestamps.
        self.number_samples = samples.len() as u32;

        for chunk in samples.chunks_exact(HOP_SIZE) {
            self.do_fft(chunk, fft);
            self.do_peak_spreading();
            if self.num_spread_ffts_done >= 46 {
                self.do_peak_recognition();
            }
        }

        if self.peaks.is_empty() {
            return None;
        }

        let uri = encode_to_uri(&self.peaks, self.number_samples, SAMPLE_RATE)?;
        let sample_ms = self.number_samples * 1000 / SAMPLE_RATE;
        Some((uri, sample_ms))
    }
}

// ── Binary encoding ───────────────────────────────────────────────────────────

fn encode_to_uri(
    peaks: &[(FrequencyBand, FrequencyPeak)],
    number_samples: u32,
    sample_rate_hz: u32,
) -> Option<String> {
    let mut buf = Vec::<u8>::new();

    // 48-byte header
    buf.write_all(&0xcafe2580u32.to_le_bytes()).ok()?;
    buf.write_all(&0u32.to_le_bytes()).ok()?; // crc32 placeholder
    buf.write_all(&0u32.to_le_bytes()).ok()?; // size_minus_header placeholder
    buf.write_all(&0x94119c00u32.to_le_bytes()).ok()?;
    buf.write_all(&0u32.to_le_bytes()).ok()?;
    buf.write_all(&0u32.to_le_bytes()).ok()?;
    buf.write_all(&0u32.to_le_bytes()).ok()?;
    let rate_id: u32 = match sample_rate_hz {
        8000 => 1, 11025 => 2, 16000 => 3, 32000 => 4, 44100 => 5, 48000 => 6, _ => return None,
    };
    buf.write_all(&(rate_id << 27).to_le_bytes()).ok()?;
    buf.write_all(&0u32.to_le_bytes()).ok()?;
    buf.write_all(&0u32.to_le_bytes()).ok()?;
    let n_field = number_samples + (sample_rate_hz as f32 * 0.24) as u32;
    buf.write_all(&n_field.to_le_bytes()).ok()?;
    buf.write_all(&((15u32 << 19) + 0x40000u32).to_le_bytes()).ok()?;

    // Root chunk (tag + size placeholder at offset 52)
    buf.write_all(&0x40000000u32.to_le_bytes()).ok()?;
    buf.write_all(&0u32.to_le_bytes()).ok()?;

    // Band chunks
    let mut by_band: std::collections::BTreeMap<FrequencyBand, Vec<&FrequencyPeak>> =
        std::collections::BTreeMap::new();
    for (band, peak) in peaks {
        by_band.entry(*band).or_default().push(peak);
    }

    for (band, band_peaks) in &by_band {
        let mut pbuf = Vec::<u8>::new();
        let mut prev_pass: u32 = 0;
        for peak in band_peaks {
            if peak.fft_pass_number.saturating_sub(prev_pass) >= 255 {
                pbuf.push(0xff);
                pbuf.extend_from_slice(&peak.fft_pass_number.to_le_bytes());
                prev_pass = peak.fft_pass_number;
            }
            pbuf.push((peak.fft_pass_number - prev_pass) as u8);
            pbuf.extend_from_slice(&peak.peak_magnitude.to_le_bytes());
            pbuf.extend_from_slice(&peak.corrected_peak_frequency_bin.to_le_bytes());
            prev_pass = peak.fft_pass_number;
        }
        let padding = (4 - pbuf.len() % 4) % 4;
        buf.write_all(&band.wire_tag().to_le_bytes()).ok()?;
        buf.write_all(&(pbuf.len() as u32).to_le_bytes()).ok()?;
        buf.write_all(&pbuf).ok()?;
        for _ in 0..padding {
            buf.push(0);
        }
    }

    let total = buf.len() as u32;
    let smh = total - 48;
    buf[8..12].copy_from_slice(&smh.to_le_bytes());
    buf[52..56].copy_from_slice(&smh.to_le_bytes());

    let mut hasher = Crc32Hasher::new();
    hasher.update(&buf[8..]);
    buf[4..8].copy_from_slice(&hasher.finalize().to_le_bytes());

    Some(format!("{}{}", DATA_URI_PREFIX, BASE64.encode(&buf)))
}

// ── PCM decoding via ffmpeg ───────────────────────────────────────────────────

pub fn decode_pcm(path: &Path) -> anyhow::Result<Vec<f32>> {
    Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|_| anyhow!("ffmpeg not found — please install ffmpeg"))?;

    let output = Command::new("ffmpeg")
        .args([
            "-i",
            path.to_str().unwrap_or_default(),
            "-f", "s16le",
            "-ac", "1",
            "-ar", "16000",
            "-loglevel", "error",
            "pipe:1",
        ])
        .output()
        .context("failed to run ffmpeg")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("ffmpeg error: {}", stderr.trim()));
    }

    let bytes = &output.stdout;
    if bytes.len() % 2 != 0 {
        return Err(anyhow!("ffmpeg produced odd number of PCM bytes"));
    }

    Ok(bytes
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
        .collect())
}

// ── Public entry point ────────────────────────────────────────────────────────

pub fn compute(path: &Path) -> anyhow::Result<Option<(String, u32)>> {
    let samples = decode_pcm(path)?;
    let n = samples.len() as u32;
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let gen = SignatureGenerator::new(n);
    Ok(gen.process_and_encode(&samples, fft.as_ref()))
}
