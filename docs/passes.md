# Multi-pass fingerprinting

Kablam! can fingerprint multiple positions within a track and pick the best match. This is controlled by the `--passes` flag (default: 3).

## Why use multiple passes?

A single 12-second fingerprint from the middle of a track can misidentify songs that contain samples, remixes, or long intros. Multiple passes sample different positions across the track, and the result with the lowest Shazam skew (highest confidence) wins.

For example, a remix that samples another song might match the sampled song at one position but correctly match the remix at most others.

## How positions are distributed

Positions are evenly spaced across the track:

| Passes | Positions sampled |
|--------|-------------------|
| 1 | 50% |
| 3 | 25%, 50%, 75% |
| 5 | 17%, 33%, 50%, 67%, 83% |
| 10 | 9%, 18%, 27%, 36%, 45%, 55%, 64%, 73%, 82%, 91% |

## Average time per file

Each pass requires one Shazam API call with a 1-second rate limit between calls. Benchmarked on a 5-minute MP3:

| Passes | Time per file | Accuracy |
|--------|---------------|----------|
| 1 | ~1s | Baseline -- may misidentify remixes/samples |
| 3 (default) | ~4s | Good balance of speed and accuracy |
| 5 | ~8s | Higher confidence for tricky tracks |
| 10 | ~17s | Maximum accuracy, best for large one-off imports |

The bottleneck is the Shazam API round-trip + rate limiting, not local fingerprint computation. Actual times may vary depending on network latency.

## Recommendations

- **Default (3)**: Good for most use cases, catches sample/remix confusion.
- **1 pass**: Use with `--passes 1` if you want maximum speed and your library has mostly straightforward tracks.
- **5-10 passes**: Use for initial imports of messy libraries where accuracy matters more than speed.
