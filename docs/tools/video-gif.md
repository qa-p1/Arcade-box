# Video to GIF / WebP (`arcade.video.gif`)

**Status:** implemented. **Input:** one video. **Output:** an animated GIF or animated WebP. **Privacy:** LOCAL via system FFmpeg.

Choose the section on the filmstrip timeline (up to 60 seconds), or type a start and duration. Other settings:
- frame rate: 1–30 fps
- width: 64–1280 px
- loop: forever or once
- quality: High / Balanced / Small

GIF output uses a two-stage palette (`palettegen` → `paletteuse`) for clean colors. The quality preset controls dithering and the palette size. WebP output needs FFmpeg's `libwebp_anim` encoder and uses the preset as its quality value. If the requested section runs past the end of the video, it ends at the last frame. A start time after the end is rejected.
