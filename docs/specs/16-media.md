# Media (HLS/DASH) specification

**Status:** parses and VOD download paths implemented; encrypted/DRM/audio-group
and parallel-segment work remains
**Code:** `crates/rustget-media`, `crates/rustget-net` (`download_hls`,
`download_dash`)

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

## 4. Implemented download path

`Downloader::download` detects a `.m3u8` or `.mpd` URL before the byte-range
probe and routes to `download_hls` or `download_dash`.

HLS:

1. GET the playlist with the normal headers, cookies and redirect policy.
2. For a master playlist, choose the highest-bandwidth variant and fetch it.
3. Reject encrypted playlists.
4. Write the `#EXT-X-MAP` init segment, then each media segment in order.
5. Rename the `.ts` part to its final name; verify an optional checksum.
6. Emit segment-count progress units while total bytes are unknown.

DASH:

1. GET and parse the MPD.
2. Choose the highest-bandwidth representation; reject `ContentProtection`.
3. Require a duration-based `SegmentTemplate` and compute the segment count.
4. Write the initialization URL, then every numbered media segment in order.
5. Produce an `.mp4` (fMP4 init + media fragments); checksum/rename as above.

Both paths use the shared retry policy, request timeouts, rate limiter and panic-
free error mapping. They are sequential today; parallel segment workers are the
next optimization.

## 4.1 Planned refinements

1. Bounded-concurrency segment workers writing to ordered scratch files.
2. Resume metadata for media (completed segment bitmap per playlist digest).
3. HLS AES-128 key fetch/decrypt and `#EXT-X-MEDIA` audio renditions.
4. DASH `SegmentTimeline`, multi-period, `$Time$` and separate audio.
5. FFmpeg `-c copy` mux for separate audio/video, then optional MP4 remux.
6. Clean temporary segments only after the final mux succeeds.

Key handling, DRM and live playlists remain explicitly out of scope for 0.1.x.

## 5. Tests

- HLS master: variant sort, relative URL, resolution/codecs.
- HLS media: init map, AES key flag, durations, ENDLIST.
- DASH: representation, BaseURL, template expansion, ISO duration.
- FFmpeg discovery does not panic when absent.
- HLS end-to-end: master → media → init → three segments, byte-exact output.
- HLS encrypted playlist rejection.
- DASH end-to-end: duration template → init + three segments, byte-exact output.
- DASH DRM rejection.
- Manager HLS routing updates the final `.ts` path/filename.

## 6. Gaps

- Parallel segment workers and resume.
- Live playlist refresh.
- Byte-range HLS.
- DASH SegmentTimeline/multi-period.
- AES-128 decryption and audio renditions.
- Mux integration test with a real FFmpeg binary.
- UI quality picker.
- Media output is raw TS/fMP4 concatenation; no remux yet.
