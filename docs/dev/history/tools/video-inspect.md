# Video / Media Inspector (`arcade.video.inspect`)

**Status:** implemented. **Input:** one audio or video file. **Output:** structured ffprobe JSON covering format, streams, chapters, codecs, timing, metadata, and technical fields. **Privacy:** LOCAL via the discovered system FFprobe paired with FFmpeg.

The Island summary shows:
- container, duration, size, and bitrate
- track counts and the chapter count
- for each stream: resolution, frame rate, bit depth, and HDR transfer (HDR10/PQ or HLG) for video; sample rate, channel layout, bitrate, language/title, and default/forced flags for audio and subtitles

Cover art is labeled as such. The raw probe JSON is available under *Show raw probe data*.

The tool does not modify the source. Very large metadata output is bounded. Unsupported or malformed media produces the provider's diagnostic behind a normal user-facing error.
