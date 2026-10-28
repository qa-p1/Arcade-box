# Join / Merge Audio (`arcade.audio.join`)

**Status:** implemented. **Input:** 2–100 audio clips, in the order shown (drag to reorder). **Output:** one combined file. **Privacy:** LOCAL via system FFmpeg.

With *Same as source*, clips that share a codec, sample rate, and channel count are joined with FFmpeg's concat demuxer. Nothing is re-encoded. The editor tells you before you run whether that will happen.

Otherwise the clips are resampled to one rate and channel layout and re-encoded. The rate comes from the first clip (or the one you choose). Mono clips are upmixed when any clip is stereo. With *Same as source* and mismatched clips, the result explains why it re-encoded.

- **Crossfade** (0–10 s) overlaps each pair of clips. Every clip must be longer than the crossfade.
- **Silence between clips** (More options) inserts a gap instead.

You can't use a crossfade and a gap together. Result metadata reports `method`.
