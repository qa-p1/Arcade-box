# File / Text Encryption

Encrypts or decrypts one selected file, or `text/plain` supplied by a pipeline or CLI, using age. Choose passphrase mode or an X25519 recipient key.

- **Input:** one file or text value; one passphrase, public recipient key, or private identity for the chosen operation.
- **Output:** an armored `.age` encrypted artifact or a decrypted artifact.
- **Privacy:** LOCAL. Content is not uploaded or added to history.
- **Safety:** writes a new output; the source remains unchanged. Cancellation and any age-format or authentication error remove the private staged output.
- **Passphrase mode:** age’s interoperable scrypt passphrase format. Use a long, unique passphrase.
- **Recipient mode:** encrypt with an `age1...` X25519 public key; decrypt with its `AGE-SECRET-KEY-...` identity. The UI accepts the corresponding key in the same field based on the selected action.
- **Provider:** age 0.12.1, age format v1. Review the selected library’s release and licensing status before shipping packaged releases.
