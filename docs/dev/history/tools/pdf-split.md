# Split / Extract PDF Pages

**LOCAL** · Requires system `qpdf` 11.9 or newer.

Enter qpdf page ranges such as `1-3,5` or `r3-r1`. Leave the range blank to select all pages. Set pages per output to `1` for one file per page, or a larger number to group pages. Outputs use sanitized names and never replace existing files.

Very large splits are capped at 10,000 output files. Split pieces may not retain document-level bookmarks, threads, and related structures.
