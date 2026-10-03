# Implementation plan

This is a living sequence. Each stage produces a usable, reviewable increment and keeps existing contracts in use.

1. **Repository contracts and audit:** workspace rules, catalog/schema, typed values/results, version rules, threat model, migration and license inventory. Verify direct and transitive licenses before distribution choices harden.
2. **Core vertical path:** registry, manifest validation, local search, SQLite migrations, job/result model, CLI invocation, and simple real tools. Keep Tauri commands narrow.
3. **Island workflow:** global shortcut abstraction, active display, compact search, expansion, keyboard accessibility, focus restoration, useful results, and dashboard discovery. Measure warm invocation.
4. **Provider broker and safety:** system provider discovery/health, structured process execution, app-managed fallback, temp artifact ownership, cancellation, logs, path and archive safety. Exercise the existing-FFmpeg acceptance case.
5. **Tool families:** implement text/data and developer utilities, then shared provider families for media, image, PDF, OCR, barcode, and archives. Build actual tools over common capabilities, not placeholders.
6. **Pipelines and background work:** typed DAG checks, artifact references, progress/cancel/retry policy, persistence/recovery, and searchable named workflows.
7. **Public extension path:** sandbox runtime/ABI decision, capability host, SDK/sample outside first-party tree, permission UI, package verification, update escalation, malicious-plugin denials.
8. **Cross-platform polish and release:** portal-backed Linux behavior, Windows/macOS adapters, accessibility/i18n/theme, signed updater, packages, SBOM/notices, performance and visual regression baselines.

## Completion gate

An item moves from planned to implemented only when a real backend works, failures are actionable, cancellation is present where applicable, output results are usable, permissions/privacy are enforced, platform limitations are documented, search metadata is present, and suitable tests/fixtures pass. Tool IDs and pipeline compatibility are reviewed before an implementation ships. Track exact status in `catalog/tools.json`; do not infer implementation from a UI card.
