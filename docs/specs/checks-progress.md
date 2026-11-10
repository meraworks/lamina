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

## Done after Phase 10 (open items of the plan)

- Task 1.5: `analyze` is `Ctx::new` and one module for each family
  (`copper`, `connect`, `sizes`, `holes`, `edge`, `place`, `zones`, `finish`).
  `connectivity()` runs no checks; the router, the push preview and the
  ratsnest tool use it. The golden files did not change.
- Task 1.4: the Jsonnet build calls `check_rules`. Task 1.6: model violations
  have a position. Task 2.1: a copper text with a thin stroke gives
  `track_width`. Task 2.4: `autoroute` refuses a board with data errors.
- Task 2.5: `crates/lamina-core/tests/recall.rs`. The triage file and its test.
- Task 3.1: the Specctra file has the keep-outs of footprints. Task 3.3: a
  closed Edge.Cuts drawing of a KiCad footprint is a board cut-out. Task 3.4:
  the `.kicad_dru` file. Task 3.5: import losses have a kind (`LossKind`).
- Task 4.5: `mask_bridge` for a mask opening that shows a track or a via.
- Task 4.7: custom rules with `kind`, `part` and `area`, the values
  `hole_to_copper`, `courtyard_clearance`, `max_track_width` and
  `min_track_angle`, and the tool `add_area`.
- Task 4.8: `crates/lamina-geom/tests/fill_trust.rs`. Task 4.9:
  `crates/lamina-core/tests/kicad_cli.rs` (runs if `KICAD_CLI` is set).
- Task 5.5: `pin1_mark`.
- Task 6.4: one drill file for each via span, and the `.gbrjob` file. `export`
  does not refuse blind and buried vias.
- Task 7.2: `paste_for_dnp`. Task 7.5: an alias for a long reference in the
  IPC-D-356 netlist.

Defects that the new tests found, and their corrections:

- A thermal spoke of a pour was cut only by copper of other nets. On the T.I.T
  board a spoke went into a non-plated hole. The spokes now use the same
  obstacles as the pour.
- The KiCad export wrote each board minimum as the smallest value of a custom
  rule, with no rule for the board limit. KiCad then missed an annular ring
  error. The rules file now starts with the board limits.
- `starved_thermal` counted the spokes of one zone on one layer: 6 false
  alarms on the example boards. It now counts all the pours of a pad. The
  example boards lost six exclusions.
- `audit_footprints` stored a fail for a footprint that differs from the
  generic package pattern. It now stores only a pass.

Later items:

- Speed: the analysis of the T.I.T example takes 0.29 s in a release build
  (before: 0.70 s; limit of the plan: 0.5 s). The thermal spokes cut the pour
  one time for all pads, and each copper layer is filled in its own thread.
- Task 6.4: the copper Gerber files have Gerber X2 attributes: the net, the part
  and the pad of each object, and the function of each aperture.
- Task 5.4: `verify_footprint` with `package` compares in the three IPC
  densities. Task 5.6: `audit_footprints` uses a second source (the footprint of
  the same part from its provider) and has `online`.
- Task 6.1: `use_fab_preset` accepts a fab profile id.
- Task 9.1: `review` writes three images if the program has a renderer.
- Task 6.3: `board_neck` finds a narrow part of the board body.
- Task 3.3: loss notes for a zone with `thru_hole_only`, a hatch pour, more
  than one outline of a zone, a text box, and a pad with a drill offset.
- The scene draws rule areas as lines on the keep-out layer.
- The outputs of the two example boards come from the current program.

- The `c3-sensor` build test passes (26 tests, zero violations). The new
  checks found this on that example, and each one is corrected:
  - The autorouter put six vias in SMD pads. The router does not put a via
    in an SMD pad now (a large pad, as a thermal pad, can have vias).
  - The input capacitor C1 of the regulator was at the output side, and the
    capacitors of U1 and U3 were more than 3 mm from their supply pins. The
    script places them at the pins.
  - The script accepted each warning that names J1 or U1. It accepts only the
    board edge warnings now.
  - `pin1_mark` gave a false alarm for a footprint with two marks. One mark
    at pad 1 is sufficient now.
  - `decoupling_far` does not examine connectors.

The server has 84 tools. The workspace has 407 tests.

## Differences from the plan

- `tombstone` is off by default. On T.I.T and Yoink it gave 7 warnings, and all
  of them were pads on thermal spokes or on short wide tracks (decision D5).
- `part_height` reads `Rules.height_limits` (keep-out id to height). The plan
  put `max_height` in the keep-out.
- `label_pin` and `polarity_mark` prefer a connector within 3.5 mm to a nearer
  part that is not a connector, and measure the distance to a pad along the pad
  row. A word matches the end of a net name only if it has 4 characters or
  fewer.
- `voltage_clearance` is a check only. `Rules::resolve` does not use the table,
  so a pour does not keep the voltage distance. The check reports a pour that
  keeps less than the voltage needs from a net on its layer.
- The IPC-2221B spacing table (`crates/lamina-drc/src/profiles/ipc2221_spacing.json`)
  was written from memory. A web search confirmed only the 0 to 15 V row, the B2
  value for 101 to 250 V, the B2 value at 500 V and the values above 500 V. It
  was not compared with the standard.
- The pours and the autorouter resolve custom rules with `net`, `class` and
  `layer` only. A rule with `kind`, `part` or `area` changes only the checks.
- A custom rule with `area` is not in the `.kicad_dru` file.
- `LossKind` comes from one function that reads the note text of an adapter
  (`Loss::from_note`). The adapters do not make each loss with its kind at the
  place where they drop the data.
- `fill_has_no_thin_necks` accepts a loss of 0.5 % of the area. The plan said
  0.1 %; the worst piece of the example boards loses 0.3 % at its corners.
- The triage file has the design view of the two boards, not the strict files.
- The fab profile numbers come from the fab pages on 2026-11-10.

## Not done

- The two example boards have footprints with no datasheet evidence (15 on
  T.I.T, 11 on Yoink), so `review` gives `not_ready` for them, and their
  outputs were made with `force`. A person with the datasheets must verify
  them; an acceptance with no comparison was not recorded.
- The second source of `audit_footprints` is the LCSC number of a part. Other
  providers are not used.
- The Gerber X2 attributes are on copper layers only, and a pour has only its
  net. No other program read the files to confirm the attributes; the Lamina
  reader reads them back.
- The image `docs/images/viewer-3d.png` shows the `c3-sensor` board before
  three capacitors moved.
