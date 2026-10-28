# Extract Frames (`arcade.video.frames`)

**Status:** implemented. **Input:** one video. **Output:** one or more PNG or JPEG images. **Privacy:** LOCAL via system FFmpeg.

| Mode | Output |
|---|---|
| Single frame | The frame at a time. Pick it on the filmstrip; the arrow keys step one frame. |
| Every N seconds | A frame at each interval between the optional start and end. |
| Every Nth frame | Every Nth decoded frame (uses the `select` filter). |

Multi-frame modes are capped by **Maximum frames** (default 500, at most 5,000). If the range would produce more frames than that, the tool saves nothing and asks you to narrow the range, sample less often, or raise the limit. Files are numbered `prefix-0001.png`, `prefix-0002.png`, …. JPEG needs FFmpeg's `mjpeg` encoder.
