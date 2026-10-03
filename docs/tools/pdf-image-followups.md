# PDF and image follow-up checklist

The catalog is the source of current status. These items remain before the PDF and image families are complete.

## PDF

- [ ] **`arcade.pdf.images-to-pdf`** — Works through img2pdf (verified on Linux); per-image crop/fill previews remain.
- [x] **`arcade.pdf.ocr`** — Works through OCRmyPDF (verified on Linux); pages that already have text are skipped.
- [ ] **`arcade.pdf.convert`** — Needs an installed LibreOffice (`soffice`); verify supported input formats on each OS.
- [ ] **`arcade.pdf.organize`** — Page thumbnails and multi-select controls remain UI work.
- [ ] **`arcade.pdf.extract`** — Reliable table reconstruction needs a mature provider.
- [ ] **`arcade.pdf.watermark`** — Text uses built-in Helvetica, so only printable ASCII is supported.
- [ ] **`arcade.pdf.sign`** — Visible signatures only; certificate-based signing is out of scope.
- [ ] **`arcade.pdf.fill`** — Text, choice, checkbox, and radio fields are filled and `NeedAppearances` is set; viewers that ignore it may show stale appearances. XFA forms are not supported.

## Images

- [ ] **`arcade.image.redact`** — Add interactive region selection and blur/pixelate modes. The backend supports confirmed solid-black replacement and OCR-based personal-info suggestions.
- [ ] **`arcade.image.background-remove` and `arcade.image.upscale`** — Provide a managed model installer only after each model's source, version, SHA-256, license, and notices are pinned. Local models are reused today.
- [ ] **`arcade.image.compare`** — Add side-by-side, overlay, and slider views; results are statistics today.

## Release verification

- [ ] Run provider-backed fixtures on Windows and macOS and record provider versions before changing platform values from `conditional`.
