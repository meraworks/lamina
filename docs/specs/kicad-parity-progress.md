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
- Task 3 checks passed and commit is `6dbc1ee`.

## Task 4

- Added NPTH/courtyard overlap checking on either side, excluding a part's own
  holes. The board-hole test failed before implementation and now passes.
- Added `min_thermal_spokes` with a default of 2. Backward compatibility test
  failed before the field existed, then passed. Model commit checks are running.
- KiCad reference cases on the copy: H3 inside BT1 courtyard; U1.44 has one
  spoke in z1. The fill code currently merges two crossing bars per thermal pad
  and does not retain counts; it needs per-pad surviving spoke evidence.
- Remaining: thermal count generation and DRC tests, part-hole tests, real-board
  comparison, all workspace checks and Task 4 commit.
- Model checks passed; commit `30695ce`.
- Both courtyard tests pass, including another part's hole and own-hole exclusion.
- Thermal test failed with no warning, then passed after adding per-pad counts
  to ZoneFill. Each clipped half-arm must have one connected piece touching the
  pad and the base pour. DRC compares the count with min_thermal_spokes.
- MCP debug build is running in exec session 95123. Next: check the copied collar board for both rules,
  review thermal counting cost and edge cases, then workspace verification.
- That build finished. Collar check took 1.8 seconds and found the expected
  h3/BT1 courtyard overlap and U1.44 one-spoke warning, but also 11 other thermal
  warnings. Evidence: `target/parity-collar-task4.json`.
- KiCad's `pcbnew/drc/drc_test_provider_zone_connections.cpp` (10.0 branch) counts
  manual tracks from the pad into the filled zone. Added a regression test that
  failed, then passed after counting those tracks. Primary source:
  https://github.com/KiCad/kicad-source-mirror/blob/10.0/pcbnew/drc/drc_test_provider_zone_connections.cpp
- The updated MCP debug build is running in session 88205. Re-run the collar
  check after it finishes; any remaining count differences need investigation.
- Updated comparison: 9 thermal warnings, including U1.44; manual tracks removed
  three extras. KiCad's refill produces different copper and its DRC skips pads
  with zero thermal connections. Lamina reports zero under this plan's minimum
  spoke rule. Remaining differences are recorded, not suppressed. Task 5's filled
  export will allow a check of Lamina's copper without KiCad refilling it.
- Debug check times before the local polygon optimization: collar 1.8 s, T.I.T
  7.24 s (includes load and all checks). Spoke tests pass after limiting polygon
  intersections to each pad's neighborhood.
- All DRC tests pass. Full workspace verification is running in session 39343.
  Task 4 commit remains; do not claim exact thermal-count parity with KiCad refill.
