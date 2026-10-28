# CSV / TSV Workbench

Reads bounded CSV, TSV, or JSON tables and can produce CSV, TSV, or JSON output. JSON input accepts an object or an array of objects; missing fields become empty cells, and nested values are rendered as JSON text.

- **Inspect:** returns the detected delimiter, column names, total row count, and the first 20 rows.
- **Transform:** sort by text or numeric value, reverse sort order, filter by a column substring, deduplicate by a column, select columns, or rename columns. Enter renames as `old = new`, one per line.
- **Limits:** input is capped at 64 MiB and one million data rows. Parsing and writing check cancellation between records.
- **Privacy:** LOCAL. Selected files are read through the user’s grant; output is a new artifact and does not replace the source.

Automatic delimiter detection parses quoted CSV fields instead of counting punctuation characters, then favors consistent record widths.
