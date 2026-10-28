# Duplicate File Finder

**Status:** Implemented (read-only). **Privacy:** LOCAL. **Input:** one or more user-selected folders. **Output:** groups of byte-identical files.

The scanner first groups files by size, then computes BLAKE3 only for candidate groups. Hashing streams file data with bounded memory. Results contain the relative path, byte count, and digest for each member.

The tool is read-only: it never deletes or moves files. Directory symlinks are skipped, traversal stays within the selected folder capability, and scans stop at 100,000 entries or when cancelled. Review groups before removing anything yourself.
