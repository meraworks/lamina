# KiCad parity progress

Follow `kicad-parity-plan.md` in task order.

## Constraints

- Keep all checks headless. Do not open a viewer window.
- Use at most two build jobs while the user works on this PC.
- Make each feature usable by an agent through MCP. Use structured data, stable
  rule names and item IDs, and errors that explain how to correct the input.
- Keep the original collar_tag board unchanged.
- Commit shared model changes separately, as required by AGENTS.md.

## Task 0

- Added a test that reads the exported `.kicad_pro` and checks a rule value.
- Added tests for a via connected to a pad and a zone with no pad connection.
- The fill engine removes zones with no same-net contact. The isolated-copper
  test uses a disconnected track as a fill anchor, then connects it to a pad.
- Added the rules and warning names to README.md.
- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -j 2 -- -D warnings`: passed.
- `cargo test --workspace -j 2`: passed. The real-board overview test is ignored.
- Headless viewer build passed. Inspected T.I.T with 9 warnings and no errors.
  Warning rows and markers disappear when Warnings is off. Screenshots are in
  `target/parity-checks.png` and `target/parity-filtered.png`.
- Model commit: `7541db4`. The remaining Task 0 changes form a separate commit.

## Remaining

Tasks 0–2 are implemented and verified. Task 3 is in progress; Tasks 4–12 are pending.

## Task 1

- Added the severity map and backward-compatible model serialization test.
- Added tests for severity changes, ignored unrouted warnings with an unchanged
  ratsnest, unknown rule names, and persisted settings.
- Observed failures for missing override behavior and rejected `severities` input.
- Added the DRC rule-name list, final severity mapping, and input validation.
- Targeted tests passed: core 23 passed, 1 ignored; DRC 24 passed.
- Formatting, workspace Clippy with warnings denied, and workspace tests passed.
- Model commit: `81bb337`. Severity behavior is committed separately.

## Task 2

- Added Exclusion records and a default empty list on Rules.
- The constructor sorts item IDs and refuses a blank reason.
- The new model test failed on the missing API, then passed after implementation.
- Model workspace checks passed; commit `c2d7185`.
- Matching, moved-item identity, stale entries, and ignored-rule interaction pass
  two DRC tests. The core exclude/include persistence test also passes.
- Added MCP exclude/include tools and a read-only exclusions tool with reasons.
- Remaining: viewer actions and filter, broader input validation tests, MCP runtime
  and headless checks, and final workspace verification.
- Added viewer Exclude/Include controls, a required reason input and Exclusions
  filter. Unknown rule names are also tested.
- Workspace Clippy and tests passed after these changes. The headless viewer and
  MCP debug binaries are building in exec session 80652 (two jobs).
- Runtime test prepared at `target/parity-mcp-check.py`; it operates only on
  `%TEMP%/lamina-parity-exclusions`, a copy of T.I.T. Run it after the build.
- Still needed: run that MCP check, inspect and exercise viewer controls with
  `shot --edit` on the temporary copy, final checks and Task 2 commit.
- MCP round trip passed: tool discovery, exclude, list reasons and hidden
  violations, include, and empty exclusion list.
- Headless viewer saved an exclusion with its reason. Reopening showed 8 warnings
  instead of 9, the saved reason, and Include. Include removed the disk entry.
  Screenshots: `target/parity-exclusion-ui.png`, `target/parity-excluded.png`,
  `target/parity-exclusions-reopened.png`, `target/parity-included.png`.
- The shot that saves an edit can show the old analysis until the watcher reloads;
  persistence and the resulting analysis were checked by reopening the copy.

## Task 3

- Added mask-opening checks against zone fills, with a bounding-box index and
  indexed polygon edges. One warning is reported per pad and zone.
- The new test failed on the missing function, then passed. It covers a no-net
  pad near a zone and quiet same-net, opposite-side, and far-away cases.
- Remaining: full DRC/integration tests, the three real-board fiducials compared
  with KiCad on a copy, workspace checks, and commit.
- All 28 DRC tests pass, including a full-analysis zone/pad integration test.
- On `%TEMP%/lamina-parity-collar`, Lamina reports FID1, FID2 and FID3 against
  zone z1. KiCad 10 reports the same three solder-mask bridges. KiCad totals:
  38 violations, 0 unconnected items (unchanged baseline).
- Evidence: `target/parity-collar-check.json` and the temporary copy's `d.json`.
- Final workspace checks and commit are in progress.
