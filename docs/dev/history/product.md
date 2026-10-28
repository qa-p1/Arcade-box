# Product direction

Arcade Box is a private, cross-platform utility surface for desktop work. Its main loop is **invoke → type → select → perform → disappear**. The launcher is the primary interface; the dashboard supports discovery, management, history, pipelines, and settings.

## Experience principles

- Search is the fastest path to an action. Users can search by name, synonym, verb/object phrase, input type, and recent context.
- The Arcade Island opens compactly at the active display, accepts text immediately, expands into the selected tool, and closes on Escape while restoring focus where the OS permits.
- Tools share controls and result actions. Results can be opened, copied, saved, revealed, or sent to another compatible tool.
- Work is local by default. Network and cloud behavior is declared and shown at the point it matters.
- Inputs are preserved by default. Tools write to a new output unless the user explicitly chooses replacement.
- Complexity appears on demand. Simple mode serves common jobs; advanced settings expose provider-specific controls when useful.

## Product boundaries

Arcade Box is not a launcher-first application, a collection of mini-app dashboards, or an online conversion service. It does not require an AI account. Optional network or cloud tools must disclose the actual provider and content flow.

## First-use flow

Onboarding welcomes the user, proposes and tests a shortcut, explains invoke/type/Enter/Escape, and offers opt-in startup and clipboard history. It then opens the Island with an invitation to try a real action. Clipboard history remains off unless explicitly enabled.

## Measures

Track repeatable local benchmarks for warm invocation, search ranking latency, startup, idle CPU, and ordinary UI memory. The design targets in the product specification are engineering goals, not achieved claims. A tool becomes implemented only when its real backend, errors, results, suitable tests, permissions, and documentation are present.
