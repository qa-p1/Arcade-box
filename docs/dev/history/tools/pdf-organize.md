# PDF Page Organizer

**LOCAL** · Requires a compatible system `qpdf` installation.

Choose a PDF, then enter output page order as a qpdf page selection such as `1,4,2-3`. Repeated numbers duplicate pages and omitted pages are removed. Rotation can apply to every output page (0°, 90°, 180°, or 270°) or to selected output page numbers after reordering. For per-page rotation, enter rules such as `1-2=90;4=180`; valid angles are 90°, 180°, and 270° clockwise. Rules cannot overlap, and page numbers must exist in the selected output. qpdf applies each rule to the specified output range using its `--rotate` page-range option ([qpdf command-line documentation](https://qpdf.readthedocs.io/en/stable/cli.html)). The original remains untouched and the output uses a new filename.

This provider-backed form does not yet show page thumbnails or multi-select controls. The qpdf provider must be available for page reordering and rotation.
