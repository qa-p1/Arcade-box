# Audio Trim / Cut (`arcade.audio.trim`)

**Status:** implemented. **Input:** one audio file. **Output:** a new audio file. **Privacy:** LOCAL via system FFmpeg.

The tool shows the file's waveform. Drag the handles to choose a range, or drag inside the range to move it. With a handle focused, the arrow keys move it 10 ms and Shift+arrow moves it 1 s. The Start/End fields stay in sync with the waveform.

- **Keep the selection** saves just the range. With *Same as source* and no fades, the audio is **copied without re-encoding**, so it keeps its original quality and is fast. Cut points then land on the codec's frame boundaries (about 20–30 ms for MP3/AAC). Turn on *Re-encode for sample-accurate cut points* for exact edges.
- **Cut the selection out** removes the range and joins the rest with a 10 ms crossfade, so the splice doesn't click. This always re-encodes.
- **Fade in / fade out** (More options) apply to the audio that's kept. Fades longer than the kept audio are rejected.

Choosing a different output format re-encodes into it. Result metadata reports `method` (`copy` or `reencode`).
