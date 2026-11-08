# KiCad parity: implementation plan

This plan is for an agent that has no earlier context. Do the tasks in order. Make one commit for each task.

## Context

Lamina is a PCB layout engine in Rust. A board is a folder of JSON files. The MCP tools are in
`crates/lamina-mcp/src/server.rs`. The DRC is in `crates/lamina-drc`. The KiCad adapter is in
`crates/lamina-kicad`. The viewer is in `crates/lamina-viewer`.

The goal is to add the basic KiCad PCB features that Lamina does not have. The reference is the
[KiCad 10 PCB Editor manual](https://docs.kicad.org/10.0/en/pcbnew/pcbnew.html).

## Rules for all tasks

Read `AGENTS.md` and obey it. The most important rules are:

- Write all text in ASD-STE100 Simplified Technical English: code comments, docs, commit messages, UI strings and errors.
- Do not add AI attribution or a `Co-Authored-By` line.
- Before each commit, run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`.
- Tests must not use the network.
- Do not run two cargo builds at the same time.
- Write a failing test first. Make sure that it fails for the correct reason.
- New fields in `board.json` must have a serde default, so that old boards load.
- Do not push, tag or release if the user did not ask for it.
- Do not change the board in `C:\Users\mranderson\Desktop\collar_tag\hardware\pcb`. Use a copy for tests.

## Task 0: commit the current work

The working tree on `master` has these changes, and they are not committed. All 210 tests pass.

- KiCad export: tracks, vias and zones write `(net N)`. Pads and holes keep `(net N "name")`.
- KiCad export: the Reference and Value fields are hidden if the footprint has no `${ref}` or `${value}` text.
- KiCad export: `BoardFormat::companions` writes a `.kicad_pro` file next to the board (`export_project`).
- New warnings: `silk_overlap`, `silk_to_edge`, `silk_text_size`, `mask_bridge`, `track_dangling`, `via_dangling`, `isolated_copper`.
- New rules: `silk_to_edge`, `min_silk_text_height`, `min_mask_web`.
- Viewer: `SceneLayer::DrcWarning` markers, and Errors and Warnings filters.

Do these steps:

1. Add a test in `crates/lamina-core/tests/ops.rs` that shows that `export_board` writes the `.kicad_pro` file.
2. Add one test for each of `isolated_copper` and a passing case of `via_dangling`. These have less cover than the other rules.
3. Add the new rules and warnings to `README.md`, near the `silk_to_pad` text.
4. Run the viewer and look at the markers and the filters. If you cannot run it, say so in your report.
5. Commit.

## How to verify with KiCad

KiCad 10.0.7 is at `C:\Users\mranderson\AppData\Local\Programs\KiCad\10.0\bin\kicad-cli.exe`.

1. Copy `hardware/pcb` to a temporary folder. A running MCP server locks the original project.
2. Run `lamina export-board --format kicad --out <tmp>/b.kicad_pcb <copy>`.
3. Run `kicad-cli pcb drc --format json --severity-all --refill-zones --output d.json <tmp>/b.kicad_pcb`.

The baseline result is 38 violations and 0 unconnected items:

| KiCad rule | Count | Task |
|---|---|---|
| `lib_footprint_issues` | 31 | 6 |
| `solder_mask_bridge` | 3 | 3 |
| `silk_overlap` | 2 | Real board warnings, for the user to decide |
| `npth_inside_courtyard` | 1 | 4 |
| `starved_thermal` | 1 | 4 |

Tasks 3 and 4 add checks to Lamina. They do not make the KiCad count smaller.

## Task 1: severity for each rule

KiCad equivalent: Board Setup > Design Rules > Violation Severity.

- Add `RuleSeverity { Error, Warning, Ignore }` in `lamina-model`.
- Add `Rules.severities: BTreeMap<String, RuleSeverity>`, with a default and `skip_serializing_if` empty.
- Add one public list of all rule names in `lamina-drc`.
- At the end of `analyze`, apply the map: change the severity or remove the violation.
- The ratsnest must not change if `unrouted` is ignored.
- Accept the map in the `set_rules` tool. Refuse an unknown rule name, and list the valid names in the error.

Tests: override to error, override to warning, ignore, unknown name, old `board.json` with no field.

## Task 2: DRC exclusions

KiCad equivalent: Exclude this violation, with a comment.

- Add `Rules.exclusions: Vec<Exclusion { rule, items, reason }>`. Sort `items` before you store them.
- Match by rule and sorted items. Do not match by position, because a position changes when a part moves.
- Add `Analysis.excluded: Vec<Violation>`. Excluded violations go there and not in `violations`.
- Add a warning `stale_exclusion` if an exclusion matches nothing.
- Add MCP tools `exclude_violation` and `include_violation`. The reason is mandatory.
- Viewer: add an Exclude action on each row and an Exclusions filter. Do not draw markers for excluded items.

Tests: a match hides the violation, the match stays after the part moves, stale entry, reason is mandatory.

## Task 3: mask openings against zones

KiCad reports that the mask opening of FID1, FID2 and FID3 exposes the GND zone. `mask_bridges` in
`crates/lamina-drc/src/silk.rs` compares only pads.

- Pass the zone fills into `silk::check`.
- For each pad with a mask opening, compare it with each fill on the same side that has another net or no net.
- Report `mask_bridge` if the distance is less than the pad mask margin plus `min_mask_web`.
- Use `Index` and bounding boxes first. A fill can have thousands of edges.

Tests: a no-net pad near a zone warns, a pad of the zone net is quiet, a far pad is quiet.

Verify: Lamina reports the same three fiducials as KiCad.

## Task 4: two more checks

`npth_inside_courtyard`: warn if a non-plated hole of one part, or a board hole, is inside the courtyard of another part on either side. The courtyards are already in `analyze` as `cys`.

`starved_thermal`: warn if a thermal pad connects to its zone with fewer spokes than `Rules.min_thermal_spokes` (default 2).
Read `crates/lamina-geom/src/fill.rs` first and find where the spokes are made. If the fill does not keep the spokes, make it return the count for each pad.

Tests for each check: one that warns and one that is quiet. Verify the result against the KiCad report.

## Task 5: filled zones in the KiCad export

- Write a `(filled_polygon (layer "F.Cu") (pts ...))` node in each zone. Use the same Y flip as the other nodes.
- KiCad filled polygons have no holes. Convert each polygon with holes to one outline with a keyhole cut for each hole.
- Make sure that the import ignores these nodes, or does not make a change to the zone outline.
- Check if `lamina-kicad` can depend on the fill code in `lamina-geom`, and do not add a dependency on an adapter crate.

Tests: a polygon with a hole converts correctly. The round trip test still passes.

Verify: KiCad DRC with no `--refill-zones` gives no unconnected items.

## Task 6: footprint library in the export

- Write each footprint to `<board>.pretty/<id>.kicad_mod`, with `companions`.
- Write an `fp-lib-table` that uses `${KIPRJMOD}`.
- The library name in the board and in the table must be the same. Today the board uses `lamina:<id>`.
- Replace characters in the footprint id that KiCad does not accept.
- Do not change the footprint at the board position to fit the library. KiCad compares the two, so the geometry must agree.

Verify: `lib_footprint_issues` goes from 31 to 0.

## Task 7: custom rules

Do not copy the KiCad rule language. Use JSON, because agents write these files.

- Add `Rules.custom: Vec<CustomRule { name, when, clearance, min_track_width, min_via_drill, min_annular_ring }>`.
- `when` has optional `net`, `class` and `layer` fields. `net` is a wildcard pattern.
- Move `wildcard` from `crates/lamina-kicad/src/project.rs` to `lamina-model`.
- If two rules match, the later rule wins.
- For a clearance between two items, use the larger value.

Tests: pattern match, layer match, order, no match uses the base rules.

## Task 8: net lengths and limits

- Add a read-only MCP tool `net_lengths`: the sum of the track lengths and the via count for each net.
- State in the output that the length is the sum of all tracks, not the pad-to-pad path.
- Add optional `length_min` and `length_max` on `Net`, and a `skew_group` with a `max_skew` value.
- Add warnings `length_out_of_range` and `skew_out_of_range`.

## Task 9: differential pairs

Do this task as two commits.

1. Data and checks: add `diff_pairs` to `nets.json` with positive net, negative net, gap and width. Add the warnings `diff_pair_gap` and `diff_pair_uncoupled`.
2. Routing: add a tool that routes both nets from one centre path. It must offset the path by half of the gap plus half of the width, and it must obey the DRC.

## Task 10: length tuning

Add a tool `tune_length` with net, target length and one track segment. Replace the segment with a serpentine of a given amplitude and spacing. Refuse the change if it adds a DRC error.

## Task 11: teardrops

The word `teardrop` is in three source files. One use is the KiCad `legacy_teardrops` token. Read these files first and report what exists.
If there is no teardrop feature, add teardrop shapes as same-net copper at via and pad connections, with size limits from the KiCad manual. Fab exports must contain them.

## Task 12: netlist and statistics

- Add an IPC-D-356 netlist file to the fab export.
- Add a read-only MCP tool `board_stats`: board size and area, part counts by side, SMD and through-hole pads, via count, drill sizes, smallest track, track length.

## Report

After each task, report the test output, the KiCad result if the task has one, and any step that you did not do. Do not say that a task is complete if a check failed.
