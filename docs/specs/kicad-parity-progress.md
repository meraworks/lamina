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
- Task 4 workspace checks passed; committed as `9a73f2a`.

## Task 5

- Added a failing export test for filled polygons and unchanged zone outlines on
  reimport. It now passes.
- KiCad depends on lamina-geom and writes fill polygons on each zone layer.
- Moved the existing Gerber keyhole converter into lamina-geom; the Gerber adapter
  delegates to it and filled polygons expose a keyhole method.
- Remaining: hole-area and wider round-trip tests, KiCad without refill on the
  collar copy (zero unconnected required), review Task 4 thermal differences on
  exported fills, workspace checks and commit.
- KiCad/export suites passed. Filled-zone test checks hole exclusion, copper area,
  Y flip and outline preservation.
- KiCad no-refill check reads the fills and reports 0 unconnected items, but 287
  violations: 214 clearance, 32 hole clearance, 4 edge clearance, and the baseline
  library/silk/mask/courtyard cases. No thermal warnings with these exported fills.
  Evidence: `%TEMP%/lamina-parity-collar/filled-drc.json`.
- Clearance examples are about 0.004 mm short. A new test proved inflated circle
  chords fell inside the requested clearance. Added the polygon tolerance to
  positive inflation. Geometry/DRC/KiCad tests are running; this fix needs a new
  no-refill comparison. Hole/edge rule differences remain to investigate.
- Tolerance fix removed all 214 copper-clearance errors. Added failing tests and
  fixes for routed-slot copper-to-edge spacing and explicit export of Lamina's
  hole clearance. Both tests pass. Next report (`filled-drc-fixed.json`) has 40
  violations, 0 unconnected, and only 3 geometric errors: slot/zone intersections.
- A diamond-hole regression proved the old nearest-vertex keyhole cut can cross
  the hole itself. Replaced it with a rightward ray from the rightmost hole point
  to the nearest boundary. Regression passes. This needs full exporter tests and
  another no-refill real-board check before commit.
- Geometry/export/KiCad tests passed. Corrected keyhole export passes KiCad DRC
  without refill with 0 unconnected and 37 violations: library 31, NPTH courtyard
  1, silk overlap 2, solder mask bridge 3. No copper/hole/edge clearance errors.
  Evidence: `%TEMP%/lamina-parity-collar/filled-drc-keyhole.json`.
- Final Task 5 workspace checks are running before commit.
- Commit/check process is exec session 70255. Clippy passed; workspace tests are
  building. The command commits only Task 5 files on success.

## Task 6 preparation

- Added `crates/lamina-kicad/tests/footprint_library.rs`. It requires a table with
  `${KIPRJMOD}/board.pretty`, safe file names, one library member for every board
  footprint (including board holes), and no placement in the library root.
- Test not run yet: wait for session 70255, then run it and observe the missing
  library failure before implementing companions and shared footprint naming.
- Task 5 checks passed and committed as `a11b97e`.
- Task 6 test failed on the missing table, then passed with companion export.
  Board and library share safe encoded IDs; long names are bounded and collision
  cases are tested. Library footprints are at the origin and include board holes.
- All 21 KiCad tests pass. Clippy passed. Workspace tests are running in session
  88815. Task 6 is not committed yet.
- Real-board no-refill DRC: 6 violations, 0 unconnected, 0 library warnings (was
  31). Remaining: NPTH courtyard 1, silk 2, mask 3. Evidence:
  `%TEMP%/lamina-parity-collar/library-drc.json`.

## Task 7: custom rules

- Task 6 workspace checks passed; committed as `b644aca`.
- Model matcher test RED to GREEN; model committed as `6c51e2e` after fmt,
  workspace clippy and workspace tests passed.
- DRC, set_rules, pour and router regression tests each failed before the
  implementation, then passed. Rules use ordered JSON selectors and fieldwise
  overrides. Clearance uses both nets; vias use all spanned layers.
- set_rules rejects blank names, unknown fields, nonpositive dimensions and
  selectors for nonexistent/non-copper layers without changing the board.
- Ruling: router profiles use the strongest resolved limit across board layers
  because a profile spans layers. This can route wider than necessary on some
  layers, but prevents layer changes from generating invalid copper.
- Full workspace verification pending. No KiCad comparison is specified for
  custom JSON rules; these rules are enforced in Lamina.
- Task 7: fmt, workspace clippy and workspace tests passed (session 30575).


## Task 8: net lengths and limits

- Model tests failed for missing fields and measurements, then both passed.
- Model and mechanical literal updates committed as `422fc78`; formatting,
  workspace clippy and workspace tests passed.
- Length/skew DRC and malformed-limit tests each failed before implementation,
  then passed. Exact bounds are accepted. Warnings name their member nets.
- `net_lengths` is read-only and states units, branch summation and excluded via
  depth. Limits are exposed through the existing typed `set_nets` schema.
- Ruling: skew is the spread between the longest and shortest total track sums;
  the strictest supplied max_skew in a group applies. This is documented in the
  model and README. It is not a propagation delay or pad-to-pad path measurement.
- Workspace checks and debug MCP build are running in session 11672. Runtime
  test script: `target/parity-lengths-mcp.py`, using its own temporary board.
- Task 8: fmt, workspace clippy/tests and debug MCP build passed. Headless MCP runtime verified read-only discovery, 5 mm measurement, length warnings and custom-width rules.

## Task 9: differential pairs

- Model round-trip failed before implementation, then passed. Model committed
  as `54f1ceb` after formatting, workspace clippy and workspace tests passed.
- Pair DRC tests failed on missing gap/uncoupled warnings, then both passed.
  Checks cover matching straight runs, wrong gap, partial, missing and wrong-layer
  partners. Core pair validation/persistence test also failed then passed.
- Named pairs use a BTreeMap. set_diff_pairs replaces it; deleting a referenced
  net is refused until the pair is removed.
- Modular-source test failed on unsupported diff_pairs, then passed with pair
  definitions generated alongside nets. Source merge combines pair maps.
- Ruling: pair checks use parallel segment coverage, 0.01 mm gap tolerance and
  one centre-spacing end allowance for corners. They do not calculate impedance.
  Short uncoupled ends within that allowance can be omitted from warnings.
- Pair routing test is prepared at target/parity-pair-route-test.rs; restore it
  under core/tests after the checks commit and run it before implementation.
- Task 9 data/checks: formatting, workspace clippy and workspace tests passed (session 90280).
- Pair data/checks committed as `85002b4`.
- Routing test failed on missing RouteDiffPair, then passed. It checks offsets
  on a right-angle path, two tracks with the specified nets, no DRC errors,
  atomic refusal at the board edge and refusal of a reversing path.
- route_diff_pair is available to MCP, always uses strict commit, and rejects
  bends that reverse or collapse the inner offset segment.
- Pair routing: added a failing cross-item ID collision regression and fixed allocation to reserve all item IDs. Targeted test, formatting, workspace clippy and workspace tests passed (session 38556).

## Task 10: length tuning

- Added tests for reaching total-net length with multiple tracks, preserving
  endpoints, invalid net/segment/dimensions, and atomic refusal of new shorts.
  Tests failed on missing API, then both passed.
- MCP tune_length accepts net, track, zero-based segment, target, maximum
  amplitude and leg spacing. It keeps endpoints, checks available space and
  minimum leg spacing, and always uses strict commit.
- Ruling: amplitude is a maximum; the final meander height is reduced to hit
  the target exactly. Meanders go left of the selected segment. The target is
  the track-length sum documented by net_lengths, not an electrical path length.
- Task 10: formatting, workspace clippy and workspace tests passed (session 58534).
