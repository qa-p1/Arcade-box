# Video Trim / Cut (`arcade.video.trim`)

**Status:** implemented. **Input:** one video. **Output:** a new clip (MP4, Matroska, WebM, or QuickTime). **Privacy:** LOCAL via system FFmpeg.

When a video is selected, the tool shows a filmstrip timeline. Drag the start and end handles, or drag the selected range to move it. With a handle focused, the arrow keys step one frame and Shift+arrow steps one second. The start/end number fields stay in sync with the timeline.

| Mode | Behavior |
|---|---|
| Auto (default) | Copies the original streams without quality loss when the start falls on a keyframe. Otherwise it re-encodes for a frame-accurate start. |
| Fast | Always copies. If needed, the start moves to the nearest earlier keyframe, and the result reports the time it used. |
| Precise | Always re-encodes, so both cut points are frame-accurate. |

Fast mode requires the target container to hold the source codec. For example, VP9 can't be copied into MP4; in that case choose Matroska or Precise. The result metadata reports `method` (`copy` or `reencode`). When a re-encode is needed, the quality preset, codec, and audio choices under **More options** apply.

The source is never modified. Output never overwrites. Progress and cancellation are supported.
