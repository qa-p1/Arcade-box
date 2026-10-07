# Vendored release tooling

`scripts/arcade-release.py` is copied without changes from
`Arcade-link/tools/arcade-release.py`, Arcade-link commit `539fa91`.
It generates the protocol-v1 installer manifest and `SHA256SUMS.txt` using
Python's standard library. Update from the shared source, preserving its
schema and classification rules; do not fork the release format here.
