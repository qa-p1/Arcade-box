# Video Join (`arcade.video.join`)

**Status:** implemented. **Input:** 2–100 videos, in the order shown (drag to reorder). **Output:** one combined video. **Privacy:** LOCAL via system FFmpeg.

In **Auto** mode the tool first checks whether the clips can be joined without re-encoding. That requires the same video codec, size, and frame rate, the same audio codec and layout, and a target container that can hold them. If they match, the clips are joined with FFmpeg's concat demuxer, which is fast and lossless. Otherwise every clip is re-encoded to a common size, frame rate, and audio format, and the result explains why. **Re-encode** mode always does this.

When re-encoding:
- Clips are scaled and letterboxed to the first clip's size (or the max height you choose).
- Clips without audio get matching silence, so sound stays in sync.
- Quality, codec, and audio options under **More options** apply.

Result metadata reports `method` (`copy` or `reencode`). Sources are never modified.
