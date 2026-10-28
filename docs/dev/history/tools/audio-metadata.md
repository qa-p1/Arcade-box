# Audio Metadata / Cover Editor (`arcade.audio.metadata`)

**Status:** implemented. **Input:** an audio file, plus an optional JPEG/PNG cover. **Output:** a re-tagged copy plus a `structured/audio-metadata` summary, or just the summary. **Privacy:** LOCAL via system FFmpeg.

**Show tags** lists the format, length, and every tag, and says whether there's cover art.

**Edit tags and cover** starts with the file's current tags filled in. It writes:
- title, artist, album, album artist
- track and disc (`3` or `3/12`)
- year (`2024` or a full date)
- genre, composer, comment

Audio streams are **copied, never re-encoded**. Supported files:

| File type | Tag format |
|---|---|
| MP3 | ID3v2.3 |
| M4A / AAC / ALAC | MP4 tags |
| FLAC | FLAC tags |
| Ogg Vorbis, Opus | Vorbis comments on the stream |
| WAV | INFO chunk (common fields only) |

- **Cover:** add a JPEG or PNG to replace it, or turn on *Remove cover art*. Covers can be embedded in MP3, M4A, and FLAC.
- **Keep only the tags filled in here** removes every other tag, including fields you cleared. Without it, tags you don't change are preserved.
