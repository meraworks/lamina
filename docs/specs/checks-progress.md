# Checks plan: progress

Follow `checks-plan.md`. The constraints of that plan apply.

## Done in 0.4.0

- Task 1.1: the plan is in `docs/specs/checks-plan.md`.
- Task 1.2: golden records in `crates/lamina-core/tests/golden`. No triage file
  yet: the two boards have no new violation.
- Task 1.3: `Violation.actual`, `Violation.required`, `measured()`. Only
  `hole_to_copper` gives the numbers now.
- Task 1.4: `crates/lamina-drc/src/registry.rs`. `RULE_NAMES` comes from it.
  `check_rules` is not called from `lamina-build` yet.
- Task 1.6: the model checks run in the analysis (`crates/lamina-drc/src/model.rs`).
  Model violations have no position yet.
- Task 1.7: strict mode counts violations with the same key.
- Task 1.8: tool `checks` (75 tools).
- Task 1.9: `release_view`, export gate, `check` with `release`, `release_errors`,
  `lamina check` exit codes. Difference from the plan: a rule that is `ignore`
  in design cannot have a release severity (`release_only` is not made).
  The export action of the viewer was not examined.
- Task 1.10: `presets::apply`.
- Task 2.1: copper texts and drawings are copper (`crates/lamina-geom/src/art.rs`).
  A stroke thinner than `min_track_width` gives no `track_width` violation yet.
  No Gerber round-trip test and no router test were added.
- Task 2.2: `hole_to_copper`: rule, check, pours, router. The KiCad `.kicad_pro`
  mapping (part d) is not done. No router test was added.
- Task 2.3: `export` refuses blind and buried vias.
- Task 2.4: `zone_outline`. `autoroute` does not refuse a board with model
  errors yet.
- Task 3.1: footprint keep-outs from KiCad. The export writes them as board
  zones. The Specctra export does not write them yet. The turn of a footprint
  with a keep-out was tested only at 0 degrees.
- Task 3.2: paste and mask openings from pads with no copper; the scene plots
  footprint and board drawings on mask and paste layers.
- Task 3.3, one line: a pad with `*.Mask` and no paste layer has no paste.

Real boards: `hole_to_copper` and the copper art change gave no new violation on
T.I.T and Yoink. The T.I.T example got its paste openings and its footprint
keep-out; its hand-made board keep-out `k2` was removed.

## Not done

- Task 1.5: the split of `analyze` into `Ctx` and family modules, and
  `connectivity()` for the router and the push preview.
- Task 2.5: the recall test file.
- Tasks 3.3 (other lines), 3.4, 3.5.
- Phases 4 to 10.
