# Image Compressor / Optimizer

**LOCAL** · Uses system libvips.

**Target file size** (the default) is for upload forms with a size limit. It searches JPEG or WebP quality from 92 down to 20, and shrinks the dimensions only if the lowest quality is still too large. The output is named after the target, such as `photo-100kb.jpg`.

**Quality** mode: JPEG and lossy WebP use the selected quality. WebP can instead use lossless encoding. PNG and TIFF use lossless encoder settings; their output may not shrink. Arcade Box reports input/output bytes and the percentage change. Metadata is preserved unless explicitly removed in the metadata tool.
