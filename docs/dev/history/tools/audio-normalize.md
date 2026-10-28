# Loudness Normalizer (`arcade.audio.normalize`)

**Status:** implemented. **Input:** 1–50 audio files. **Output:** a normalized file plus a `structured/audio-loudness` report per input. **Privacy:** LOCAL via system FFmpeg.

**Loudness (default)** is a true **two-pass EBU R128** normalization. The first pass measures integrated loudness, true peak, loudness range, and threshold. The second pass applies `loudnorm` with those measurements in linear mode, which is a plain gain change that keeps the dynamics.

Presets:
- Streaming music: −14 LUFS
- Podcast: −16 LUFS
- Speech/audiobook: −18 LUFS
- Broadcast EBU R128: −23 LUFS
- Custom: −36 to −6 LUFS

The true-peak limit defaults to −1 dBTP. If the target can't be reached within the peak limit, FFmpeg switches to dynamic processing, and the result warns about it. The original sample rate is restored after processing.

**Peak level** measures the sample peak with `volumedetect` and applies one gain so the loudest sample hits the target (default −1 dBFS).

**Before running:** the editor measures the file's current loudness (files up to 10 minutes are measured automatically; longer ones have a **Measure** button). It shows the gain needed for the chosen target.

**After running:** the result shows loudness before and after, the true peak after, and the target. Silent files are rejected. Output defaults to the source's own format at a similar bitrate.
