# Pipelines

Pipelines are saved, typed workflows that use the same tools and runtime as direct actions. The scheduler is DAG-capable even if the first editor presents a simple linear sequence. Nodes reference stable tool IDs and version/API expectations; edges connect compatible output and input types. Branching, batch input, conditions, reusable variables, progress, retries, and cancellation are model features, not separate backend implementations.

## Execution rules

1. Validate the DAG for missing tools, incompatible types, cycles, unavailable permissions, and provider requirements before starting.
2. Resolve variables and user-provided inputs into scoped values.
3. Schedule independent nodes subject to resource limits; preserve declared ordering for dependent nodes.
4. Pass artifact/file references between stages. Keep intermediate artifacts in a private job directory with explicit ownership and cleanup metadata.
5. Publish structured progress and stage status at a bounded rate. On cancellation, stop child work, close handles, and remove partial outputs unless a stage explicitly supports preserving them.
6. Record completion, safe output references, errors, and warnings in local history according to user retention settings.

Each stage declares whether it is deterministic, batch-capable, and safe to retry. A retry cannot blindly repeat a non-idempotent network or destructive side effect. A pipeline never silently writes over source files; replacement is an explicit output choice.

## Versioning and UX

Saved pipelines have their own version and reference stable tool IDs. On an incompatible tool update, mark the affected stage, offer a documented migration when possible, and otherwise keep the workflow intact but blocked until repaired. Saved pipeline names and aliases are indexed like tool names; a dedicated shortcut is a later convenience. The editor optimizes for short repeatable tasks rather than exposing a programming canvas by default.
