## Changes in 0.5.0

The checks plan is complete: Lamina finds the board defects that KiCad finds,
defects that fab and assembly tools find, and defects that no layout tool
finds (wrong footprints, wrong labels, silent import losses).

### New checks

- **From KiCad:** `via_count`, `courtyard_missing`, `courtyard_open`,
  `weak_connection`, `track_zero_length`, `track_angle`, `zone_overlap`,
  `mask_missing`, `silk_on_hole`, `silk_outside_board`, `silk_line_width`,
  `text_variable`. `drill_size` and `annular_ring` also examine board holes.
- **Fab:** `hole_to_edge`, `drill_aspect`, `mask_sliver`, `board_neck`.
- **Assembly and BOM:** `fiducial_count`, `part_to_edge`, `part_height`,
  `paste_missing`, `paste_oversize`, `via_in_pad`, `testpoint_access`,
  `bom_conflict`, `bom_part_number`, `tombstone` (off by default).
- **Labels:** `silk_under_part`, `label_far`, `label_pin`, `polarity_mark`.
  They read what the silkscreen says and compare it with the parts and nets.
- **Footprints:** `footprint_mismatch`, `footprint_stale`, `footprint_changed`,
  `footprint_unverified`, `footprint_symmetry`, `pin1_mark`.
- **Electrical, with no schematic:** `net_single_pad`, `pad_no_net`,
  `track_current`, `voltage_clearance`, `decoupling_far`, `return_path`.
- `import_loss`: data that an import dropped stays an open item.

### New tools (84 in total)

- `verify_footprint`, `audit_footprints`, `compare_footprints`: evidence for
  each footprint, kept in the project. `export` refuses a board that has a
  footprint with no evidence, if `force` is not true.
- `board_minimums`, `fab_profiles`, `fab_compat`: the smallest features of a
  board against the limits of JLCPCB and PCBWay. `use_fab_preset` accepts a
  fab profile.
- `review`: one verdict before an order, with images, in `output/review.md`.
- `apply_fixes`: applies the fixes that the checks propose.
- `add_area`: a rule area for custom rules.

### Other changes

- Custom rules select by item type (`kind`), part and area, and have the values
  `hole_to_copper`, `courtyard_clearance`, `max_track_width` and
  `min_track_angle`. The pours and the autorouter use them.
- Command line: `lamina review`, `lamina diff`, and
  `lamina check --format text|json|sarif|junit`.
- Fab output: one drill file for each span of blind or buried vias, a
  `.gbrjob` file, Gerber X2 net and part attributes on copper, BOM columns
  Manufacturer and MPN, and the CPL position at the middle of the pads.
  Parts that are not placed (DNP) get no paste.
- KiCad: a `.kicad_dru` file with the custom rules, rule areas in the two
  directions, board cut-outs from footprint Edge.Cuts drawings.
- A violation has `actual`, `required` and, where one is safe, a `fix`.
- The analysis is faster: 0.3 s for the T.I.T example (before: 0.7 s).

### Fixed

- A thermal spoke of a pour could go into a hole, a keep-out or a different
  pour. Spokes now keep clear of all that the pour keeps clear of.
- The autorouter put vias in SMD pads. It does not do that now.
- `starved_thermal` counted the spokes of one zone on one layer. It now counts
  all the pours of a pad.
- KiCad export: a custom rule with a smaller limit made KiCad miss errors of
  the board limit.

### Old boards

- The pours of a board can change: thermal spokes are cut where they went into
  holes or keep-outs, and a pour keeps the voltage distance if nets have a
  `voltage`. Export the fabrication files again.
- A board with a footprint that has no evidence needs `audit_footprints`,
  `verify_footprint` or `force: true` to export.
- `export` does not refuse blind and buried vias now.

### Limits

- The voltage spacing table agrees with one secondary source for IPC-2221C.
  It was not compared with the standard. Do not use `voltage_clearance` as the
  only check for a safety distance.
- The record of the work is in `docs/specs/checks-progress.md`.
