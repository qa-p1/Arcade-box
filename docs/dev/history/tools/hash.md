# Hash / Checksum

**Status:** implemented. **Privacy:** LOCAL. **Input:** text or a user-selected file. **Output:** lowercase hexadecimal digest text.

Choose SHA-256, SHA-512, or BLAKE3. File hashing reads in 1 MiB chunks and checks cancellation between chunks; the file is never loaded entirely into UI memory. Selected files are identified by scoped Arcade tokens, and source paths/content are absent from normal job history. The same core implementation is available from `arcadebox run arcade.developer.hash --file path/to/file --algorithm sha256`.

The digest is a checksum, not a claim about the file's trustworthiness. Verify expected values from a trusted source. Available on the platform when Arcade Box can read a user-selected local file; frontend file selection still requires platform validation.
