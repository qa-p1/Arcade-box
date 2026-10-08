# File / Folder Compare

**Status:** Implemented (read-only). **Privacy:** LOCAL. **Input:** two or more selected files, or two or more selected folders. **Output:** a structured difference report.

File comparison streams BLAKE3 hashes and compares both size and digest. Folder comparison reports missing and changed relative paths across the selected roots. This is a byte-level comparison; it does not render images or compare document meaning.

The operation is read-only. Folder scans skip symlinks and are limited to 100,000 entries per root. For large files, hashing may take time but does not load the entire file into memory.
