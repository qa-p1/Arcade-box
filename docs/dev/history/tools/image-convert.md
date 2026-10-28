# Image Format Converter

**LOCAL** · Uses the system libvips provider.

Convert PNG, JPEG, WebP, TIFF, HEIC/HEIF, AVIF, GIF, and BMP images (whatever the installed libvips can load) to PNG, JPEG, WebP, or TIFF. HEIC photos from iPhones convert to JPEG this way. JPEG output places transparent pixels on white; images without transparency are not flattened. The source remains unchanged and output names never overwrite an existing file.

Input support depends on the installed libvips loaders (HEIC and AVIF need libheif). Animated GIFs are converted from their first frame.
