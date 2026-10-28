# ADR 0005: Main application license and dependency audit

- **Status:** Provisional; the workspace currently declares `GPL-3.0-or-later`, pending complete dependency and distribution audit.
- **Problem:** The source license must match the open-source goal, dependency graph, plugin boundaries, and release packaging obligations.
- **Decision:** Retain GPL-3.0-or-later as the working application license until formal review. Maintain a machine-readable inventory of direct and resolved third-party dependencies, SPDX expressions, sources, notices, and distribution obligations.
- **Reason:** GPL-3.0-or-later is a reasonable candidate in the product brief, but no final compatibility conclusion should be inferred from only direct dependencies.
- **Consequences:** Before release, resolve and audit every target dependency (including feature-selected and platform-specific crates/npm packages), check static/dynamic/provider bundling obligations, emit required notices, and reject incompatible additions. The current inventory is provisional until lockfiles and packaged dependency closure exist.
