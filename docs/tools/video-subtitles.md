# Subtitle Tool (`arcade.video.subtitles`)

**Status:** implemented. **Input:** a video and/or a subtitle file (SRT, ASS/SSA, VTT, SUB). **Output:** subtitle files or a new video. **Privacy:** LOCAL via system FFmpeg.

With a video selected, the tool lists its subtitle tracks (language, title, format). Image-based tracks (PGS, VobSub, DVB) are marked. Click a track to choose it.

| Action | Result |
|---|---|
| Save embedded subtitles | Saves the chosen track, or **every track**, as SRT, VTT, or ASS. Image-based tracks can't be saved as text, and the tool says so. |
| Convert a subtitle file | Converts a selected subtitle file between SRT, VTT, and ASS. |
| Add a subtitle track | Adds the selected subtitle file as a soft track, with an optional language code. Matroska keeps the existing tracks; MP4/MOV use `mov_text`, WebM uses WebVTT. |
| Burn subtitles into the picture | Draws a subtitle file or an embedded track into the video. Text tracks use the `subtitles` filter. Image-based tracks are composited with `overlay`. The video is re-encoded with the chosen quality. |
| Remove all subtitle tracks | Copies video and audio without any subtitle tracks. |

Adding and removing tracks copy the video and audio without re-encoding. The original is never modified.
