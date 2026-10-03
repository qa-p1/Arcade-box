# Video Convert (`arcade.video.convert`)

**Status:** implemented. **Input:** 1–50 videos. **Output:** one new video per input (MP4, Matroska, WebM, or QuickTime). **Privacy:** LOCAL via system FFmpeg.

Pick a container, a quality preset, and optionally a maximum height. The defaults are H.264 + AAC for MP4/MKV/MOV and VP9 + Opus for WebM. Under **More options** you can choose the video codec (H.264, H.265, AV1, VP9, ProRes), set an exact CRF, copy or drop the audio, and choose the output name.

| Preset | H.264 CRF | H.265 CRF | AV1 / VP9 CRF |
|---|---|---|---|
| High | 18 | 20 | 24 |
| Balanced | 23 | 26 | 32 |
| Small | 28 | 31 | 40 |

ProRes uses profile HQ / Standard / Proxy instead of CRF. A max height scales the video down only. It never upscales, and it keeps the aspect ratio with even dimensions. The tool rejects codecs the container cannot hold before FFmpeg starts (for example ProRes in WebM). When a codec's encoder isn't in the installed FFmpeg, that choice shows as *not installed* and can't be selected.

**Batch:** with several videos selected, each one is converted with the same settings. A file that fails becomes a warning, and the other files continue. Custom output names are ignored in batch mode. Each output takes the source name plus `-converted`.

Sources are never modified. Outputs never overwrite an existing file. Progress and cancellation are supported.
