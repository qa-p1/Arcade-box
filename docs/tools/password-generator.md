# Password / Passphrase Generator

Generates random passwords from the selected character set or pronounceable pseudo-word passphrases. Random choices use the operating system seeded cryptographic RNG. The current passphrase words are generated syllable combinations, not dictionary words.

- **Input:** generator settings.
- **Output:** generated text plus estimated generator entropy metadata.
- **Privacy:** LOCAL. The generated value is returned only to the current result and is not written to job history or logs.
- **Limits:** password length 8–128; passphrase length 4–12 pseudo-words. Use a password manager to store the result.
- **Provider:** Rust `rand` backed by the system CSPRNG.
