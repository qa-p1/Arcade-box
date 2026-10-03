# Video Compress (`arcade.video.compress`)

**Status:** implemented. **Input:** 1–50 videos. **Output:** one smaller video per input. **Privacy:** LOCAL via system FFmpeg.

| Mode | How it works |
|---|---|
| For sharing (default) | Discord (under 10 MB), WhatsApp (under 16 MB), email (under 25 MB), or Discord Nitro Basic (under 50 MB). Scales to at most 720p, caps video at 2.5 Mb/s, and uses the target-size encode to land just under the limit. |
| Quality | Constant-quality encode with the High / Balanced / Small preset. |
| Target size | Works out the video bitrate from the duration, the target in MB, and the audio bitrate, leaving about 2% for container overhead. It then runs a **two-pass** encode for H.264, H.265, and VP9. AV1 uses a single bitrate-limited pass. |
| Resolution | Scales down to 2160p…360p and encodes at the chosen quality. |

**Estimate:** with one video selected, target-size mode shows the expected size immediately. Quality and resolution modes offer an **Estimate size** button. It encodes three 4-second samples (or the whole clip if it is 15 s or shorter) with the current settings and extrapolates. It is an estimate, not a guarantee.

A target too small to hold at least 64 kb/s of video is rejected, and the error gives the smallest workable size. If the result is not smaller than the source, the tool says so instead of claiming savings. ProRes can't be used with a target size. Result metadata includes `sourceBytes` and `twoPass`.

**Batch:** each selected video is compressed with the same settings. Failures become warnings.
