# Search and context

Search metadata comes from tool manifests and installed plugins. Do not maintain a separate frontend catalog. Index canonical/short names, aliases, descriptions, curated phrases, verbs, nouns, input/output types, category, and documentation examples. The first release can use local token, prefix, edit-distance, and phrase matching; no remote LLM is required.

## Ranking

Rank exact tool ID/name and strong phrase matches above fuzzy matches. Combine token coverage/order, alias and natural phrase matches, action/object compatibility, compatible current inputs, recently used tools, favorites, and frequency. Favorites and usage improve a weak match but never bury an exact result. Keep exact long-tail tools reachable even when they have no usage history. Calculator expressions and future quick parameters use explicit parsers with a normal tool fallback.

Search should remain local and effectively instant. Build indexes off the UI path, update incrementally when registry/aliases/history change, and measure p95 query latency for the full catalog. Keep score features inspectable in diagnostics and test representative utterances such as “join document”, “make jpg smaller”, “video audio”, and “qr screen”.

## Context

On invocation, inspect only locally available and user-approved context, such as clipboard MIME type, selected files, active-window metadata, or recent screen result. Context inspection is on demand; there is no continuous remote clipboard analysis. Clipboard history is a separate opt-in feature with expiry and exclusions. Context suggestions explain why they appeared and never auto-open a URL or destructive action.

The search resolver combines context with query results. A copied JSON value can suggest formatting/conversion, a URL can suggest download or redirect inspection, and an image can suggest resize/OCR/QR actions. Sensitive values must not be copied into search logs or analytics.
