# Searchable PDF OCR

**LOCAL** · Requires Tesseract (with its language data), Poppler and qpdf.

Select a scanned PDF and enter installed language codes such as `eng` or `eng+fra`. Poppler renders each page that has no text yet at 300 DPI, Tesseract writes an invisible text layer, and qpdf lays it over the original pages, which are otherwise untouched. The result is a new searchable PDF; nothing is uploaded.

If Tesseract is missing, **Engines & dependencies** offers a per-user download shared by every Arcade app (Linux x86_64 and Windows); otherwise install it and its language data through the system package manager. This form does not yet enumerate installed languages or offer image cleanup (deskew, despeckle).
