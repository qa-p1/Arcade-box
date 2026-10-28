# Extract Audio from Video (`arcade.video.extract-audio`)

**Status:** implemented. **Input:** 1–50 videos. **Output:** one audio file per input. **Privacy:** LOCAL via system FFmpeg.

Formats: MP3, M4A (AAC), FLAC, WAV, Ogg Vorbis, Opus, or **Original (no re-encode)**. Lossy formats use the bitrate you set.

*Original* copies the audio track exactly and picks a matching file type:

| Source audio | Saved as |
|---|---|
| AAC/ALAC | `.m4a` |
| MP3 | `.mp3` |
| Opus | `.opus` |
| Vorbis | `.ogg` |
| FLAC | `.flac` |
| PCM | `.wav` |

Other codecs (AC-3, DTS, …) can't be copied into a common audio file, so they are converted losslessly to FLAC, and the result says so.

With one video selected, the tool lists its audio tracks with language, codec, and channels. Click one to choose it. The first track is 0. A video with no audio fails with a clear message. In batch mode, failures become per-file warnings.
