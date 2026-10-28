# Compress / Optimize PDF

**LOCAL** · Requires system `qpdf` 11.9 or newer. Lossy image compression requires qpdf 12.1+ with the `--optimize-images` and `--jpeg-quality` capabilities detected.

**Under a file size** (the default) is for upload limits. It needs Ghostscript (`gs`). Images are resampled from 200 DPI down to 48 DPI until the file fits, so text and vector graphics stay sharp. If even 48 DPI is too large, the error gives the smallest size reached.

**Lossless structure** It uses object streams and Flate recompression, preserves embedded image pixels, and reports input/output size. Image-heavy PDFs may not shrink and can become larger.

**Balanced image compression** recompresses eligible embedded images at JPEG quality 82. **Smaller file** uses quality 60. **Custom JPEG quality** accepts 30–95. qpdf only keeps supported recompressions when they reduce image-stream size; it does not downsample image resolution. JPEG recompression is lossy and may introduce artifacts, including when a source image was already JPEG. The result may still be larger than the input. Review it before replacing or sharing it.
