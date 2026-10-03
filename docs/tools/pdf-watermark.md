# PDF Watermark / Stamp / Page Numbers

**LOCAL** · Requires compatible system `qpdf` and Poppler `pdfinfo`.

Choose a text mark, an image mark, or a supplied PDF overlay. Text marks use built-in Helvetica and support printable ASCII plus `{page}` and `{pages}` placeholders. Image marks accept local image files and preserve transparency. Text and image marks provide 1–100% opacity, seven page positions, and page ranges such as `1-3,5`; leave page ranges empty to mark every page. The source PDF stays in place and the result is published under a new, unused filename.

For image mode, select the source PDF first and the image second. For PDF overlay mode, select the source PDF first and the overlay PDF second. PDF overlay mode repeats its first page over the selected source pages and retains that overlay PDF's existing appearance; opacity and position controls apply to text and image marks.

The handler creates an exact-size transparent overlay page for each source page, so mixed page sizes retain their positions. It rejects unsupported text glyphs and oversized or malformed page ranges. qpdf 11.9 or newer is required by provider discovery for page and overlay selection. Provider availability and behavior still need release fixture verification on Windows, macOS, and Linux.
