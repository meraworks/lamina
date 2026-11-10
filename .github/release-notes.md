## Changes in 0.4.0

Checks that find more, and an export that refuses a board that is not complete:

- New check `hole_to_copper`: a hole wall near copper of another net, and a
  non-plated hole near copper. Before, a track through a mounting hole gave no
  violation. Pours and the autorouter keep the same distance.
- Texts and drawings on copper layers are checked as copper. Before, they were
  in the Gerber files, but no check, pour or router saw them.
- `export` refuses a board with open connections or parts with no placement,
  if `force` is not true. It always refuses blind and buried vias, because the
  drill files have only through holes.
- New tool `checks`: each check with what it reports, why, its severity and the
  rule fields that set its limits. `check` has `release: true` to see the board
  as `export` sees it, and `status` has `release_errors`.
- `lamina check` has `--release`, `--fail-on` and `--rules`, and exit code 2 on
  violations, for use in CI.
- The model checks (for example `pin_role`) accept severities and exclusions.
  Checks of data that is not valid stay errors. New check `zone_outline`: a
  zone or board outline that is not valid gives a violation and does not stop
  the program.
- A violation has `actual` and `required` values where a check compares a
  number with a limit (first in `hole_to_copper`).
- KiCad import keeps the keep-out zones of a footprint (antenna areas) and the
  paste and mask openings that a footprint has as pads with no copper. The
  KiCad export writes footprint keep-outs.
- `use_fab_preset` keeps the custom rules, exclusions and severities.
- `strict` counts violations that have the same key, so a second free track
  end is refused.

Fixed in the examples: `examples/tit-flight-computer` had no paste on the
ground pad of the ESP32 module. Its antenna keep-out is now in the footprint.

Old boards: `hole_to_copper` is 0.2 mm for a board that does not set it. A pour
with a smaller clearance moves away from non-plated holes. A board with open
connections now needs `force: true` to export.

Not in this release: the other checks of `docs/specs/checks-plan.md`.
