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

- `audit_footprints` compares a footprint that came with a KiCad board with
  the footprint of that name in the KiCad library (exact pads). With the
  KiCad 10 library of this computer: all 14 footprints of Yoink agree, and 23
  of 25 of T.I.T. `review` gives `ready_with_warnings` for Yoink.
- A second program (the Python library `gerbonara`) read a copper Gerber file
  of the T.I.T example: 862 objects, 42 nets, and the aperture functions
  `Conductor`, `ViaPad`, `ComponentPad` and `SMDPad`. It gives no attributes
  for regions, so the pads that are regions were not confirmed that way.
- `fill_has_no_thin_necks` has the bound of the plan: a piece loses less than
  0.1 % of its area, or less than a quarter of the square of the minimum
  width for a very small piece. The worst piece of the examples is at 6 % of
  the limit.

- A third program (the Python library `pygerber`) read the same file and
  gives the attributes of regions: 199 of the 301 regions have the pad, the
  part and the net (for example `U6,8`, `U6`, `SPI_SCK`).
- The voltage distance is in the pour and in the router: a pour keeps the
  distance that the voltage between its net and a different net needs, and
  the router uses the largest distance that the voltage of a net needs. The
  table is in `lamina-model` (`voltage.rs`).
- The pour resolves the custom rules for each obstacle with its type, its
  part and its area, and for the zone with `kind: zone`.
- A keep-out has `max_height`, and `add_keepout` sets it.
- The export action of the viewer calls the same `export` operation, so it
  has the same release gate.

- The autorouter uses the custom rules for item types, parts and areas: an
  obstacle gets the clearance of its type, part and area, and a net gets the
  strongest track and via limit of all layers and areas.
- A rule area goes to KiCad as a rule area with its name, and a rule with
  `area` is in the `.kicad_dru` file (`A.intersectsArea`). KiCad 10 applies
  it: the comparison test has one clearance violation in an area, and the two
  programs report it. The import reads the area back.

- The T.I.T example is corrected. Its USB-C socket had four pads 0.05 mm to
  one side of the library position (A1 and B12 at -3.15 mm, A12 and B1 at
  +3.25 mm; the library has -3.2 and +3.2). The pads are at the library
  position now, and `footprint_symmetry` reports nothing. The ESP32 module has
  an acceptance: its pads are those of the library, and the drill of the
  ground vias is 0.3 mm in place of 0.2 mm, which is the minimum drill of the
  board. All 25 footprints have evidence, the design check has no violation,
  `review` gives `ready_with_warnings` (BOM lines with no part number), and
  the outputs were made with no `force`. The files of the owner in
  `Desktop\tit` were not changed: the same defect is in them.

- Each importer (KiCad, EasyEDA, Gerber) gives a loss its kind at the place
  where it drops the data (`lost(notes, losses, kind, text)`). No function
  reads the note text to find the kind.

- The verification step of Phase 5 was done on a copy of a private board with
  its first footprints, with the numbers of the datasheets. `verify_footprint`
  gave a fail for each of the three footprints that a review by hand found
  wrong: two holes that are too small for the lead (`leads`), and one land
  pattern with different pad positions and sizes (`pads`). That board is not
  in this repository.

The server has 84 tools. The workspace has 412 tests.

## As the plan says, for the record

- `tombstone` is off by default. Decision D5 of the plan: a rule with fewer
  than 7 true findings of 10 on the real boards defaults to `ignore`. On
  T.I.T and Yoink it gave 7 warnings, and none was a true defect.

## Differences from the plan

- `label_pin` and `polarity_mark` prefer a connector within 3.5 mm to a nearer
  part that is not a connector, and measure the distance to a pad along the pad
  row. A word matches the end of a net name only if it has 4 characters or
  fewer.
- The voltage spacing table (`crates/lamina-drc/src/profiles/ipc2221_spacing.json`)
  has the values of IPC-2221C (2023), Table 6-1, columns B1, B2 and B4, as the
  source code of the `pcb-toolkit` crate gives them. The first version of the
  table came from memory of IPC-2221B and had smaller values for B2 and B4. The
  table was not compared with the standard itself, which is not free.
- The triage file has the design view of the two boards, not the strict files.
- The fab profile numbers come from the fab pages on 2026-11-10.

## Not done

These items need a document that this work did not have.

- The voltage spacing table agrees with one secondary source. It was not
  compared with IPC-2221 itself, which is not free.
- The footprint evidence of the two examples says that the pads are those of
  the KiCad 10 library (and one acceptance with its reason). No footprint was
  compared with the datasheet of its part.
- The image `docs/images/viewer-3d.png` shows the `c3-sensor` board before
  three capacitors moved.
