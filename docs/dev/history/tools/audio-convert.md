# Audio Format Converter (`arcade.audio.convert`)

**Status:** implemented. **Input:** 1–50 audio files. **Output:** one converted file per input. **Privacy:** LOCAL via system FFmpeg.

| Format | Encoder | Notes |
|---|---|---|
| MP3 | `libmp3lame` | Up to 320 kb/s and 48 kHz. ID3v2.3 tags for wide player support. |
| M4A (AAC) | `aac` | Cover art and iTunes-style tags. |
| FLAC | `flac` | Lossless. |
| WAV | PCM | 16-bit, or 24-bit when the source has more than 16 bits. |
| Ogg Vorbis | `libvorbis` | Tags are kept as Vorbis comments. |
| Opus | `libopus` | Always 48 kHz (Opus' native rate); default 128 kb/s. |

**More options** set the sample rate (22.05–96 kHz) and channels (mono/stereo). Tags are carried over, including Ogg stream comments moving into MP3/M4A/FLAC. Cover art is kept when the target can hold it (MP3, M4A, FLAC). When the target can't, for example Ogg/Opus/WAV, the result says the cover was dropped.

Formats whose encoder is missing from the installed FFmpeg show as *not installed*. The first audio track is converted. Batches report failures per file, and the other files continue. Sources are never modified, and outputs never overwrite.
