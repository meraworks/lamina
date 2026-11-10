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

## Done after 0.4.0 (branch `checks-phases`)

- Phase 4: `via_count`, `courtyard_missing`, `courtyard_open`, `courtyard_clearance`,
  `weak_connection`, `track_zero_length`, `track_angle` (off by default),
  `zone_overlap`, `mask_missing`, `silk_on_hole`, `silk_outside_board`,
  `silk_line_width`, `text_variable`; board holes in `drill_size` and
  `annular_ring`. Tasks 3.3 (the `.kicad_pro` minimums), 3.4 (the hidden Fab
  reference) and 3.5 (`import_loss` from open import notes).
- Phase 5: `pads_hash`, evidence in the footprint file, `compare_footprints`,
  `verify_footprint`, `audit_footprints`, and the rules `footprint_mismatch`,
  `footprint_stale`, `footprint_changed`, `footprint_unverified`,
  `footprint_symmetry`. `release_only` exists now.
- Phase 6: fab profiles as JSON (JLCPCB, PCBWay), `board_minimums`,
  `fab_profiles`, `fab_compat`, and the rules `hole_to_edge`, `drill_aspect`,
  `mask_sliver`, `board_neck`.
- Phase 7: `fiducial_count`, `part_to_edge`, `part_height`, `paste_missing`,
  `paste_oversize`, `via_in_pad`, `testpoint_access`, `bom_conflict`,
  `bom_part_number` (release), `tombstone`. BOM columns Manufacturer and MPN,
  `exclude_from_bom`, the CPL position at the middle of the pads, `cpl_rotation`.
- Phase 8: `silk_under_part`, `label_far`, `label_pin`, `polarity_mark`;
  `Violation.fix`; `apply_fixes`.
- Phase 9: `review` (tool and CLI, `output/review.md`), `lamina check --format
  text|json|sarif|junit`, `lamina diff`.
- Phase 10: `net_single_pad`, `pad_no_net` (off by default), `track_current`,
  `voltage_clearance`, `decoupling_far`, `return_path`; `NetClass.reference_plane`.

The server has 83 tools. The workspace has 374 tests.

## Differences from the plan

- `tombstone` is off by default. On T.I.T and Yoink it gave 7 warnings, and all
  of them were pads on thermal spokes or on short wide tracks: fewer than 7 of 10
  were true (decision D5).
- `part_height` reads `Rules.height_limits` (keep-out id to height). The plan
  put `max_height` in the keep-out.
- `label_pin` and `polarity_mark` prefer a connector within 3.5 mm to a nearer
  part that is not a connector, and measure the distance to a pad along the pad
  row. A word matches the end of a net name only if it has 4 characters or
  fewer. Without these rules the checks missed the real Yoink defects or gave
  false alarms on the T.I.T servo labels.
- `voltage_clearance` is a check only. `Rules::resolve` does not use the table,
  so a pour does not keep the voltage distance. The check reports a pour that
  keeps less than the voltage needs from a net on its layer.
- The IPC-2221B spacing table (`crates/lamina-drc/src/profiles/ipc2221_spacing.json`)
  was written from memory. A web search confirmed only the 0 to 15 V row, the B2
  value for 101 to 250 V, the B2 value at 500 V and the values above 500 V. It
  was not compared with the standard.
- `verify_footprint` with `package` compares with the nominal IPC pattern. It
  has no band from the least to the most density.
- `audit_footprints` has no second-source mode and no online mode.
- The fab profile numbers come from the fab pages on 2026-11-10.
  `use_fab_preset` does not use the profiles.
- `review` adds no images.
- Time of the analysis of T.I.T in a release build: 0.70 s. The limit of the
  plan is 0.5 s. The pour takes 0.69 s of it; the new checks take about 15 ms.
  The time of the pour before this work was not measured.

## Not done

- Task 1.5: the split of `analyze` into `Ctx` and family modules, and
  `connectivity()` for the router and the push preview.
- Model violations have no position.
- Task 2.5: one recall test file. The recall tests are in the test file of each
  family (`crates/lamina-drc/tests/labels.rs` has the three label defects).
- `crates/lamina-core/tests/golden/triage.txt`.
- Task 3.3: Edge.Cuts drawings of a footprint as cut-outs; typed losses
  (`LossKind`). Task 3.4: the `.kicad_dru` file. Specctra part keep-outs.
- Task 4.7 (custom rules, second version), Task 4.8 (the two tests of the
  pour), Task 4.9 (the comparison with `kicad-cli`).
- `mask_bridge` against tracks and vias.
- `pin1_mark`.
- Task 6.4: one drill file for each via span, Gerber X2 attributes, the
  `.gbrjob` file. `export` still refuses blind and buried vias.
- Task 7.2: `paste_for_dnp`. Task 7.5: the long refdes alias in IPC-D-356.
- `board_neck` examines only the width of cut-outs.
- The two example boards have footprints with no evidence, so `review` gives
  `not_ready` for them.
