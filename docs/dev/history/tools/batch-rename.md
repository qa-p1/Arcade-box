# Batch Rename

**Status:** Implemented. **Privacy:** LOCAL. **Input:** 1–5,000 user-selected files. **Output:** a rename plan and, after explicit selection, renamed copies.

Preview text replacement, regular-expression replacement, numbering, prefix/suffix, lowercase/uppercase, and **Make names safe**. Make names safe replaces characters and reserved names that break on every system, or only on the chosen one (Windows, macOS, or Linux). It replaces the old filename sanitizer tool. The preview contains the final generated names. Turn on **Create renamed copies** to write copies into private artifacts or a chosen destination.

The tool does not rename originals in place. It preserves each source file and creates new files with collision-safe names. Filenames are checked against portable desktop restrictions. Regex matching uses Rust's linear-time regex engine; look-around and backreferences are not available.
