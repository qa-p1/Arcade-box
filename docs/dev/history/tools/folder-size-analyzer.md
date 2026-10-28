# Disk / Folder Size Analyzer

**Status:** Implemented (read-only). **Privacy:** LOCAL. **Input:** one user-selected folder. **Output:** total bytes, file count, largest files/directories, and bytes grouped by extension.

The analysis uses file metadata and never reads file contents. It follows the selected capability, skips symlinks, and stops at 100,000 entries. Access-denied files can make a report incomplete; the tool does not request administrator privileges.
