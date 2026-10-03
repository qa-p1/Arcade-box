# Merge PDFs

**Status:** partial, usable. **Privacy:** LOCAL. **Input:** 2–128 user-selected PDFs in the order shown. **Output:** one new PDF next to the first input.

Select PDFs, reorder them with drag or keyboard move controls, and choose the output filename. Arcade Box verifies PDF content signatures and scoped file grants, then uses a compatible system qpdf provider. The first PDF is qpdf's primary input, so its document metadata and bookmarks are retained where qpdf supports them. Bookmarks from later PDFs may not be preserved. Existing files are never overwritten; a numeric suffix is added on a name collision.

The backend was exercised with qpdf 12.4.1 against two generated one-page PDFs and verified the merged output contains two pages. The remaining work before this tool is release-ready includes page previews, broader fixture coverage, and platform-specific desktop validation. If qpdf is missing, the tool explains the dependency in its error state.

The qpdf page-selection behavior is documented in the [official qpdf manual](https://qpdf.readthedocs.io/en/latest/cli.html).
