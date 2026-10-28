# ADR 0003: Capability-based provider broker

- **Status:** Accepted.
- **Problem:** Tools need mature engines without hardcoding executable paths, duplicating compatible global installations, or exposing arbitrary shell execution.
- **Decision:** Tools declare provider capabilities. A broker discovers, verifies, selects, and invokes system or Arcade-managed providers through typed adapters and structured arguments.
- **Reason:** One safe discovery/fallback path scales across media, OCR, PDF, archive, and conversion tools.
- **Consequences:** Each provider needs tested minimum capabilities, provenance, version/license data, health checks, and platform coverage. Managed copies are app-scoped; system installations are never silently changed.
