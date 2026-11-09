# Arcade Box v1 decisions

Decisions the plan left open, made by the v1 session. Each one cites the part of catalog `dev/v1/README.md` or
`PLAN.md` it follows. Newest last.

## D1 — Stable promotion waits for the signing secret (blocker B1)

The `ARCADE_UPDATE_SIGNING_KEY` Actions secret does not exist and this session cannot create it (catalog
`dev/v1/PROGRESS.md` B1, Link and Tools D1). rc releases on the hidden `test` channel need no signature, so all rc work
and every rc exit gate continue; `promote.yml` refuses to run without the secret. README: "never publish unsigned
stable".

## D2 — Spec and audit copied at catalog `a33fc70`

`docs/v1/SPEC.md` is `specs/BOX.md`. `docs/v1/audit/` holds `box.md` with its text evidence (`box/`), `box-real.md`,
`FINDINGS.md`, `memory.md` (with Box's measurement files in `memory-box/`) and `conformance-baseline.md`, all from
catalog commit `a33fc70`. Screenshots stay in the catalog (PLAN: text only). Line references in the spec are for Box
0.4.3 (`497c160`).
