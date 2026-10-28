# Silence Trim / Split (`arcade.audio.silence`)

**Status:** implemented. **Input:** one audio file. **Output:** a new file, or numbered parts plus a `structured/audio-segments` list. **Privacy:** LOCAL via system FFmpeg.

The editor runs `silencedetect` with your threshold and minimum length and marks every silent stretch on the waveform. It updates as you change the settings and tells you what the action will do.

| Action | Result |
|---|---|
| Trim silence at the start and end | Cuts leading and trailing silence, leaving *padding* (default 0.1 s) around the sound. |
| Shorten long pauses | Shortens every silence to *Keep of each pause* (default 0.3 s), cutting inside the silence so no sound is clipped. Useful for speech. |
| Split into parts at silence | Saves each stretch of sound as `name-01`, `name-02`, …, with padding. Limited to 500 parts. |

A file that's entirely below the threshold is rejected with a hint to lower it. Split results include start and end times for every part.
