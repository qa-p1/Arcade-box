# PDF Content Extractor

**LOCAL** · Requires Poppler (`pdftotext`, `pdfinfo`, `pdfimages`, and `pdfdetach`).

Returns extracted reading-order text, document metadata, and structured listings of embedded images and attachments. By default it also saves embedded image streams beside the selected PDF and returns them as file results. Turn off **Save embedded images** to inspect without publishing those image files. **Save embedded attachments** is off by default; when enabled, each attachment is saved as a generated `.bin` file, never using the embedded name as a path. Displayed embedded names have path components removed and unsafe characters replaced.

Table layout is not reconstructed; tables appear as ordinary text where detectable. Attachments are extracted one at a time into private staging and are bounded to 100 files, 128 MiB per file, and 512 MiB total. Output files must be regular files and are published without overwriting. Text extraction is bounded to 8 MiB. Image extraction is bounded to 500 files, 300 million listed source pixels, 256 MiB per output, and 1 GiB total published image bytes. Poppler provider fixtures and cross-platform release verification remain required.
