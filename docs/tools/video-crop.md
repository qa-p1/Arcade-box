# Video Crop / Resize / Rotate (`arcade.video.crop`)

**Status:** implemented. **Input:** 1–50 videos. **Output:** one edited video per input. **Privacy:** LOCAL via system FFmpeg.

With one video selected, the tool shows a frame from the middle of the clip. Drag on it to draw a crop area, or drag an existing area to move it. When an aspect ratio is chosen, the area keeps that ratio. **Reset area** goes back to the centered preset. The drawn area fills the advanced *Crop width/height/X/Y* fields.

- **Aspect ratio:** without a drawn area, the tool crops the largest centered region with that ratio. Ratios: 16:9, 9:16, 1:1, 4:3, 3:4, 4:5, 21:9.
- **Rotate:** 90°, 180°, or 270°. Rotation is applied after cropping.
- **Flip:** horizontal or vertical. This needs FFmpeg's `hflip`/`vflip` filters.
- **Output width/height:** resize after cropping. If you set only one, the other follows the aspect ratio.

A crop area that goes past the frame is rejected with the frame's size in the error. Odd sizes are rounded to even values, because most encoders need that. Audio is copied or re-encoded according to **More options**.

**Batch:** aspect, rotate, flip, and resize apply to every selected video. A drawn area is only available with one video. Failures become per-file warnings.
