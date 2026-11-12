# Media (HLS/DASH) specification

**Status:** parsers and FFmpeg boundary implemented; download orchestration not
wired
**Code:** `crates/rustget-media`

## 1. HLS

The parser accepts master and media playlists:

- `#EXTM3U` required;
- `#EXT-X-STREAM-INF` variants with `BANDWIDTH`, `AVERAGE-BANDWIDTH`,
  `RESOLUTION`, `CODECS`;
- relative URIs resolved against the playlist URL;
- `#EXT-X-TARGETDURATION`, `#EXT-X-MEDIA-SEQUENCE`;
- `#EXTINF` duration (fractional seconds);
- `#EXT-X-MAP` initialization segment;
- `#EXT-X-KEY` marks AES-128/other encryption and records the key URI;
- `#EXT-X-ENDLIST` for VOD.

Segments preserve URL, duration, sequence and optional byte-range (the byte-range
tag parser is a planned extension; the model has the field).

## 2. DASH

The parser accepts an MPD with:

- `mediaPresentationDuration` in the `PT#H#M#S` subset;
- `<BaseURL>` at MPD, Period, AdaptationSet and Representation levels;
- `<AdaptationSet>` inheritance of `mimeType`/`codecs`;
- `<Representation>` id, bandwidth, width, height;
- `<SegmentTemplate>` initialization/media/startNumber;
- `$RepresentationID$`, `$Bandwidth$`, `$Number$`, `$Number%05d$` expansion for
  initialization URLs.

Representations are sorted by bandwidth. Timeline construction, SegmentTimeline,
`$Time$`, multi-period and DRM are not implemented.

## 3. FFmpeg boundary

- `find_ffmpeg` checks `RUSTGET_FFMPEG`, then `PATH`, without a shell.
- `mux_copy(video, audio, output)` uses an argument vector and `-c copy`,
  `+faststart`.
- The binary is never bundled.
- No URL is passed to FFmpeg; local segment files are muxed.
- A nonzero exit returns a bounded stderr excerpt.

## 4. Planned orchestration

1. Probe the manifest URL with the normal engine headers/cookies.
2. Parse master/variant or MPD.
3. Download initialization segment + media segments with bounded concurrency.
4. For AES-128 HLS, fetch the key over HTTPS, decrypt, verify sequence.
5. Concatenate TS segments or fMP4 init+media.
6. For separate audio/video, invoke FFmpeg `-c copy`.
7. Verify the final mux and optionally remux to MP4.
8. Delete temporary segments only after success.

Key handling, DRM and live playlists are explicitly out of scope for 0.1.0.

## 5. Tests

- HLS master: variant sort, relative URL, resolution/codecs.
- HLS media: init map, AES key flag, durations, ENDLIST.
- DASH: representation, BaseURL, template expansion, ISO duration.
- FFmpeg discovery does not panic when absent.

## 6. Gaps

- Segment downloader and retry policy.
- Live playlist refresh.
- Byte-range HLS.
- DASH SegmentTimeline/multi-period.
- AES-128 decryption (requires a crypto dependency and a key cache).
- Mux integration test with a real FFmpeg binary.
- UI quality picker.
