# Images to PDF

**LOCAL** · Requires `img2pdf` (install with `uv tool install img2pdf`). JPEG images are embedded without re-encoding, so there is no quality loss.

Select and reorder image files. Choose original image-sized pages, A4, or Letter. Fixed-size pages can set orientation, image placement, and margins in millimeters. For original image-sized pages, choose an optional DPI from 72 to 600; margins on original-sized pages require an explicit DPI so their physical dimensions are defined. Otherwise img2pdf uses embedded image density metadata/defaults. The source images are not changed; the PDF is written beside the first selected image.

Per-image crop/fill previews and a DPI override for fixed paper sizes are not included. Formats depend on the installed `img2pdf` build. Review output page sizes before printing.
