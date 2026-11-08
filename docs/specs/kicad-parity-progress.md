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

Tasks 0 and 1 are complete. Tasks 2–12 are pending.

## Task 1

- Added the severity map and backward-compatible model serialization test.
- Added tests for severity changes, ignored unrouted warnings with an unchanged
  ratsnest, unknown rule names, and persisted settings.
- Observed failures for missing override behavior and rejected `severities` input.
- Added the DRC rule-name list, final severity mapping, and input validation.
- Targeted tests passed: core 23 passed, 1 ignored; DRC 24 passed.
- Formatting, workspace Clippy with warnings denied, and workspace tests passed.
- Model commit: `81bb337`. Severity behavior is committed separately.
