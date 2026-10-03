# Searchable PDF OCR

**LOCAL** · Requires OCRmyPDF and its local OCR engine/language data.

Select a scanned PDF and enter installed language codes such as `eng` or `eng+fra`. OCRmyPDF skips pages that already contain text and writes a new searchable PDF. It does not upload the document.

Install OCRmyPDF with `uv tool install ocrmypdf` (Arch's repositories don't package it) and Tesseract language data through the system package manager. This form does not yet enumerate installed languages or expose image cleanup modes.
