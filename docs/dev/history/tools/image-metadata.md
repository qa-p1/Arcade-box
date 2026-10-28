# Image Metadata Inspector & Sanitizer

**LOCAL** · Uses system libvips.

Inspect local image metadata or create a cleaned copy. Sanitizing applies EXIF orientation to the pixels first, then writes a new file with metadata stripped. It cannot restore the removed properties. JPEG is re-encoded at quality 100 and may undergo a small lossy change; the source file remains unchanged.
