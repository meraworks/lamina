# Lamina checks: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to do this plan task by task. Steps use checkbox (`- [ ]`) syntax.

## Context

The collar board review showed where Lamina is weak. The worst defects were wrong footprints, and no DRC program finds those. Other defects were silent: the KiCad import dropped an antenna keep-out, and a label named the pins of a header in the reverse order. The user wants Lamina to become an industry-level PCB program, and names the checks as the most important part. This plan says what to build and in which order. It changes no code.

For this plan I inspected:

- **Lamina:** all of the check code (`crates/lamina-drc`, `crates/lamina-core/src/validate.rs`), the model, the pour, the import and export code.
- **KiCad 10:** the 62 DRC items of a project file, the custom rule constraints and the 51 ERC items in the KiCad 10 source.
- **Other tools:** the rule lists of Altium Designer (65 rules), Autodesk Fusion (EAGLE), EasyEDA Pro, Cadence DesignTrue DFM, Valor NPI, DFMStream, JLCDFM and HQDFM.
- **Fabs:** the capability pages of JLCPCB and PCBWay.
- **Newer tools:** AllSpice, Flux, Quilter, JITX, atopile, tscircuit, HyperLynx DRC.

Two reviewers then examined the draft against the code: one the engine design, one the list of new checks. Their corrections are in this plan.

**A correction.** Earlier in this session I said that KiCad found 139 errors on the collar board that Lamina did not find. I examined the report again. 134 of them had one cause: I had made a Lamina custom rule that permitted a smaller via ring on GND, and the KiCad export does not write custom rules. 4 came from a different keep-out definition for non-plated pads. 1 was in the Lamina report also, as a warning. So that number does not show a missing check. The gaps in section 1 come from the code.

**Goal:** Lamina finds each board defect that KiCad 10 finds, the defects that fab and assembly tools find, and defects that no tool finds today (wrong footprints, wrong labels, silent import losses).

**Architecture:** One check engine in `lamina-drc` with a registry of checks, one pipeline for all rules, and results with numbers. Fab limits are data, not code. New tools use core operations, as the current tools do. Ports and adapters stay as they are.

**Tech stack:** Rust workspace, `geo` and `rstar` (in `lamina-geom`), `rmcp` and `schemars` (MCP tools), `serde` JSON project files.

**Spec:** sections 1 and 2 of this file, and `docs/specs/2026-11-05-lamina-design.md`.

**Repository:** `C:\Users\mranderson\projects\lamina`, branch `master`, commit `f096ab2`, version 0.3.0. All paths are relative to it.

## Decisions made for you (change any of them)

| # | Decision | Reason |
|---|---|---|
| D1 | Checks first. Netlist import, microvias, track arcs, impedance, panels and IPC-2581 are in section 5 as later work. | You named the checks as the most important part. |
| D2 | Fab-critical checks are errors. An error stops `export`. The tables give the severity of each rule. | You want errors to be found. |
| D3 | Some rules are stricter for release. `export` and `check` with `release: true` use the release view. | During placement, open connections are normal. At export, they are errors. |
| D4 | A footprint in use with no evidence is an error for release. `force: true` or a recorded acceptance with a reason lets the export continue. | Wrong footprints were the worst defects, and no other check finds them. |
| D5 | After each new check, run it on the real boards. Record each new violation as true or false. | This is the test for false alarms. A rule with too many false alarms stays off by default. |
| D6 | KiCad stays out of the product. One ignored test compares Lamina with `kicad-cli` when the variable `KICAD_CLI` is set. | The design document has the anti-goal "a dependency on KiCad, its libraries or its CLI". A `cross_check` tool is possible later, if you want it. |
| D7 | Fab limits are JSON data with a source URL and the date of the last check. | Fab pages change. The current presets differ from the pages of today. |
| D8 | Checks that guess (labels, tombstone, decoupling) are warnings and never errors. | They can be wrong. |
| D9 | Order: the engine, then the holes in the current checks, then KiCad parity, then the footprint evidence. Import and export corrections (Phase 3) are a second track in their own worktree. | The engine makes each later check small. Phase 3 touches other crates and prevents wrong boards now. |

## Global constraints

- Read `AGENTS.md` and obey it.
- Write all text in ASD-STE100 Simplified Technical English: comments, docs, commit messages, tool descriptions, errors.
- Do not add AI attribution or a `Co-Authored-By` line.
- Before each commit: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace -j 2`.
- Do not run two cargo builds at the same time. Use at most two build jobs while the user works on the PC.
- Tests must not use the network. Do not open a viewer window; use `lamina-viewer shot`.
- Write a test that fails first. Make sure that it fails for the correct reason.
- A new field in a project file must have a serde default, so that old boards load.
- A change to `lamina-model` types goes in its own commit.
- `lamina-core` and the domain crates must not name a vendor, a file format or an engine. Add a port method in `lamina-ports`, implement it in the adapter, plug it in `lamina-wiring`.
- One commit for each task. Do not push, tag or release if the user did not ask for it.
- Rule ids are stable and only grow. The `items` of a violation hold ids (refdes, pad refs, track ids, footprint ids). They never hold positions, piece numbers or segment numbers, so that exclusions stay valid.
- Do not change the board in `C:\Users\mranderson\Desktop\collar_tag\hardware\pcb`. Use a copy. Do not put that board in the Lamina repository.
- When you add a tool, change the tool count in `README.md` (line 41, "74 tools"). The documents test `doc_check` fails if you do not.

## Review focus

1. **Old boards.** A `board.json` without the new fields must load. The old rules must report the same violations as before.
2. **False alarms.** Each new warning on the real boards must be a true finding. If it is not, correct the check or keep it off by default.
3. **Speed.** The analysis runs after each change, and the push preview of the viewer runs it on each pointer move. The full analysis of T.I.T must stay below 0.5 s in a release build.
4. **Pours are trusted.** Checks skip pour pieces against other nets, because the fill keeps the clearance. A fill defect passes with no error. Task 4.8 measures the fill in a test.
5. **Bad data.** A board with a model error (an outline with a bad number, a track on a layer that does not exist) must give a violation. It must not stop the program.

---

## 1. Findings

### 1.1 Lamina today

- 31 DRC rules in `lamina_drc::RULE_NAMES` and 11 model checks in `validate.rs`.
- `analyze` in `crates/lamina-drc/src/lib.rs` is one function of 750 lines. A check is a block of code in it.
- A violation has a message, a position, a layer and item ids. It has no numbers (measured value, limit).
- The 11 model checks have no severity setting, no exclusion and no marker in the viewer.
- Fab limits are three Rust functions (`presets.rs`). One board has one set of limits.
- `lamina check <dir>` always exits with 0.

### 1.2 Holes in the current checks (each one is in the code now)

| # | Hole | Evidence |
|---|---|---|
| H1 | Text and drawings on copper layers go to the Gerber files, but no check, no pour and no router sees them. A copper text can short two nets with 0 errors. | `lamina-scene/src/lib.rs:379-409` plots them; `lamina-geom/src/board.rs:237-384` and `fill.rs` do not read them |
| H2 | No check compares a drilled hole with copper. A track can go through a non-plated hole, and the autorouter can route one there. | `BoardGeometry.drills` is used only for `hole_to_hole` and `npth_inside_courtyard` (`lamina-drc/src/lib.rs:569-601, 691-715`); `lamina-route/src/lib.rs:496-503` |
| H3 | `export` writes fab files for a board with open connections. It gives only a warning. `status` shows 0 errors for that board. | `lamina-core/src/ops/output.rs:124-126`; `lamina-drc/src/lib.rs:401-407` |
| H4 | Blind and buried vias go into the one through-hole drill file. The fab drills them through the board. | `lamina-export/src/lib.rs:93-142` |
| H5 | `use_fab_preset` deletes the custom rules, the exclusions and the severities of the board. | `lamina-core/src/ops/board.rs:264-285` |
| H6 | KiCad import drops keep-outs that are in a footprint (antenna areas). Export to KiCad and Specctra does not write footprint keep-outs. | `lamina-kicad/src/footprint.rs:254`, `pcb.rs:997-1186`, `lamina-specctra/src/lib.rs:156` |
| H7 | KiCad import drops pads that have only paste or only mask. Footprint drawings on paste and mask layers are never plotted. **The published example `examples/tit-flight-computer` has no paste on the ground pad of the ESP32 module (U1.41) for this reason.** | `lamina-kicad/src/footprint.rs:240-244, 560-563`, `lamina-scene/src/lib.rs:241-248` |
| H8 | Copper drawings in a footprint never reach the Gerber files, the checks or the pour. | `lamina-scene/src/lib.rs:241-248`, `lamina-kicad/src/layers.rs:67` |
| H9 | KiCad import drops footprint drawings on Edge.Cuts (slots) and pads with a drill offset. It reads some values wrong: `pad_to_mask_clearance` 0, the paste flag with `*.Mask`, `thru_hole_only`. It does not read `min_clearance`. | `lamina-kicad/src/layers.rs:74`, `footprint.rs:475-487, 599-605`, `pcb.rs:62-69, 322-327`, `project.rs:19-35` |
| H10 | The import shows its losses one time and does not keep them. The viewer discards them. An export does not report what it cannot write. | `lamina-core/src/ops/format.rs:176-178, 261-264`, `lamina-viewer/src/main.rs:2652` |
| H11 | KiCad export writes a visible silkscreen reference at the size of the Fab text, writes the copper clearance as the hole clearance, and writes no custom rules. | `lamina-kicad/src/pcb.rs:1020-1083`, `project.rs:142-156` |
| H12 | A part without a courtyard drawing gets a box around its pads. No warning tells the user. | `lamina-geom/src/board.rs:387-420` |
| H13 | Strict mode compares sets of keys. A second violation with the same key (the other free end of one track) is not "new", so `strict` accepts it. | `lamina-core/src/project.rs:262-268`; `lamina-drc/src/lib.rs:897-904, 936-944` |
| H14 | No check examines a zone outline. A bad outline goes directly to the polygon operations. The router uses a layer number with no limit. | `lamina-geom/src/fill.rs:150-156, 179`; `lamina-route/src/lib.rs:117-126` |
| H15 | The drill and ring checks read footprint pads only. They do not read `board.holes`. | `lamina-drc/src/lib.rs:497-567` |
| H16 | Silkscreen that is fully outside the board gives no warning. The edge check looks only near an edge. | `lamina-drc/src/silk.rs:124-160` |
| H17 | Two copper items that touch at one point (0.000001 mm) count as a connection. | `lamina-drc/src/lib.rs:264-268` |
| H18 | BOM: no MPN and no manufacturer column; `exclude_from_bom` has no effect. CPL: the position is the footprint origin, not the part centre; no rotation correction. | `lamina-export/src/lib.rs:153-209` |

### 1.3 KiCad 10 DRC items (62 in a project file) against Lamina

| Status | KiCad items |
|---|---|
| Lamina has it (31) | annular_width, clearance, copper_edge_clearance, courtyards_overlap, diff_pair_gap_out_of_range, diff_pair_uncoupled_length_too_long, drill_out_of_range (minimum only), hole_to_hole, holes_co_located (as `hole_to_hole` at 0 mm), invalid_outline, isolated_copper, item_on_disabled_layer, items_not_allowed, length_out_of_range, skew_out_of_range, npth_inside_courtyard, pth_inside_courtyard (as `courtyard_overlap`), padstack (as `annular_ring`), shorting_items, tracks_crossing, silk_edge_clearance, silk_over_copper (pads only), silk_overlap, solder_mask_bridge (pads and pours only), starved_thermal, text_height (silkscreen only), track_dangling, via_dangling, track_width (minimum only), unconnected_items (as a warning), missing_footprint |
| Cannot occur in Lamina (7) | duplicate_footprints, through_hole_pad_without_hole, mirrored_text_on_front_layer, nonmirrored_text_on_back_layer, text_on_edge_cuts, text_thickness, copper_sliver (the fill removes thin necks) |
| Needs a schematic or a feature that Lamina does not have (11) | extra_footprint, footprint_symbol_mismatch, footprint_symbol_field_mismatch, footprint_filters_mismatch, net_conflict, footprint_type_mismatch, footprint (net ties), microvia_drill_out_of_range, track_on_post_machined_layer, missing_tuning_profile, tuning_profile_track_geometries |
| **Missing (13)** | hole_clearance, connection_width, missing_courtyard, malformed_courtyard, lib_footprint_mismatch, lib_footprint_issues, zones_intersect, too_many_vias, track_angle, track_segment_length, unresolved_variable, creepage, track_not_centered_on_via |

- This plan adds 11 of the 13. Phase 10 adds the voltage distance of `creepage`; the path along the surface is later work. `track_not_centered_on_via` is not in the plan: KiCad ignores it by default, and it has no effect at the fab.
- KiCad also reports `via_diameter`. The via drill and ring limits of Lamina imply it, and the fab comparison of Task 6.2 has the via diameter.
- KiCad custom rules have about 30 constraint types with conditions on item type, area and part. Lamina custom rules have 4 values and 3 selectors (net, class, layer). Task 4.7 extends them.

### 1.4 Checks of other tools that KiCad does not have

| Source | Checks |
|---|---|
| Altium, Fusion | acute angle (acid trap), minimum mask sliver, silkscreen to mask opening, net antennae, vias under SMD, test points, component height, return path, parallel segment, maximum track width |
| Valor NPI, DFMStream, Cadence DFM | acid traps, mask slivers, duplicate drills, drill to board thickness ratio, hole to edge, missing mask opening, missing paste, paste area ratio, pad covered by copper (tombstone risk), fiducials, part to part distance table, test point access |
| HyperLynx DRC, Quilter, Flux | return path, nets across plane gaps, decoupling capacitor distance, capacitor voltage rating |
| Fab pages | a different hole to track limit for each hole type, slot widths, text height, V-cut edge distance |

### 1.5 What no tool has (the new features of this plan)

1. **Footprint evidence.** Compare each footprint with the numbers of the datasheet and with a second source. Keep the evidence in the project. Stop the export when a footprint has none.
2. **One board, many fabs.** Measure the smallest features of a board one time. Compare them with JLCPCB, PCBWay and other profiles. Show which fab accepts the board, and why not.
3. **Label checks.** Find a label that is far from its part, pin labels in the reverse order, a polarity mark on the wrong pad.
4. **Losses are never silent.** Each item that an import drops stays as an open item until someone accepts it.
5. **Results that an agent can use.** Each violation has the measured value, the limit and a fix that a tool can apply.
6. **A verdict.** One `review` call gives "ready" or "not ready" with the evidence, as a file for the person who places the order.
7. **Electrical checks without a schematic.** Track current, voltage distance, decoupling distance and return path, from net data.

## 2. Engine design

```
ProjectData ─> lamina_geom::build ─> Ctx ─> families ─> finish ─> Analysis
               (pads, tracks, vias,  (pair scan,  (one module   (exclusions,
                holes, copper art)    indexes)     each)         severities)
                                       │
                                       └─> connectivity()  for the router and the push preview (no checks)
```

- `crates/lamina-drc/src/registry.rs`: one `CheckInfo` for each rule. `RULE_NAMES` comes from it. `registry::violation(id, message)` makes a violation with the severity of the registry, so each rule has one place for its severity.
- `crates/lamina-drc/src/ctx.rs`: `Ctx::new` does the pair scan of today (`lib.rs:221-268`) one time and keeps the near pairs with their distances. Each family is one module with `pub(crate) fn run(ctx: &Ctx, out: &mut Vec<Violation>)`.
- The checks of `validate.rs` move into `lamina-drc` as the family `Model`. Then all rules have severities, exclusions and markers. Structural rules are not `overridable` and not `excludable`.
- `Violation` gets `actual: Option<f64>` and `required: Option<f64>`. `key()` does not change.
- A rule can be `ignore` in design and have a release severity. Its violations go to `Analysis.release_only`. `release_view(&Analysis, &Rules)` gives the list for release: a pure step, no second analysis.
- `Analysis` keeps its fields and these invariants, because the router uses them: `geometry.copper` has no pour pieces; `island[i]` is for `geometry.copper[i]`; `fill_island` aligns with `fills`.
- The map from Lamina rule ids to KiCad names is in `lamina-kicad`, not in `lamina-drc`.

Code to use again (do not write it a second time):

| Need | Use | Where |
|---|---|---|
| Distance between two shapes | `Prim::distance` (exact, 0 when they touch) | `crates/lamina-geom/src/prim.rs:72` |
| Near items | `Index::new`, `Index::near(&Rect, margin)` | `crates/lamina-geom/src/lib.rs:31-60` |
| Touch test for a pour piece | `IndexedPoly::touches`, `touches_poly` | `crates/lamina-geom/src/prim.rs:357-460` |
| Make a shape larger or smaller | `fill::inflate(&Prim, d)`, then `geo::BooleanOps` | `crates/lamina-geom/src/fill.rs:30-45` |
| Pads of a placed part | `placed_pads(fp, pl, layers)` | `crates/lamina-geom/src/board.rs:211` |
| Courtyard and part keep-outs | `courtyard(fp, pl)`, `part_keepouts(fp, pl, layers)` | `crates/lamina-geom/src/board.rs:387, 451` |
| Text as strokes | `font::side_text_strokes`, `font::text_width`, `font::silk_stroke_width` | `crates/lamina-geom/src/font.rs:265-331` |
| Limits for a net, class and layer | `Rules::resolve` | `crates/lamina-model/src/rules.rs:33-72` |
| Name patterns | `wildcard(pattern, name)` | `crates/lamina-model/src/rules.rs:75` |
| Role of a pin from its name | `pin_role(name)` | `crates/lamina-model/src/footprint.rs:140-196` |
| IPC-7351 land sizes from package dimensions | `ipc::land`, `build::generate`, `PackageSpec`, `presets::resolve` | `crates/lamina-footprint/src/ipc.rs`, `build.rs:11-121`, `presets.rs:88-444` |
| Files next to an exported board | `BoardFormat::companions` | `crates/lamina-ports/src/format.rs:58` |
| Test boards | `board()`, `only()`, `text()`, `track()` | `crates/lamina-drc/tests/drc.rs:5-37, 445-460` |
| Time of the analysis | `cargo run --release -p lamina-drc --example timing -- <dir>` | `crates/lamina-drc/examples/timing.rs` |
| KiCad fixtures | `ESP32-S3-WROOM-1` (keep-out, 9 paste pads), `QFN-32-1EP_5x5mm_P0.5mm_EP3.45x3.45mm` | `crates/lamina-kicad/tests/fixtures/footprints` |

## 3. Phases and tasks

Phases 1 to 3 have task detail. Phases 4 to 10 have the decisions (rule ids, fields, values, tests). Write their steps when their turn comes, because they use the types that Phases 1 and 2 make. Each phase gives a version of Lamina that works.

Two tracks, each in its own worktree: track A is Phases 1, 2, 4 and later (`lamina-drc`, `lamina-geom`, `lamina-core`). Track B is Phase 3 (`lamina-kicad`, `lamina-scene`, `lamina-export`, `lamina-ports`). Task 3.5 needs the registry of Task 1.4.

**The pattern for each new check** (the tasks do not repeat it):

- [ ] If the check needs a limit: add the field to `Rules` with a serde default (`crates/lamina-model/src/files.rs:155-224`), in a model commit. The serde default is the value that old boards get. Use the JLCPCB standard value, or 0 (off) if no common value exists.
- [ ] Add the `CheckInfo` row to `registry.rs`.
- [ ] Write the tests in `crates/lamina-drc/tests/<family>.rs`, one small board for each: 0.001 mm inside the limit fails; exactly at the limit passes; the result does not change when the board turns 90, 180, 270 and 37 degrees, or when the part goes to the bottom side; the keys are unique; an exclusion hides the violation.
- [ ] Write the check in the family module. Use `registry::violation` and `.measured(actual, required)`.
- [ ] Add the limit to the fab profiles if it is a fab limit.
- [ ] Run the timing example on `examples/tit-flight-computer`. Record the time.
- [ ] Run the check on the real boards (section 4). Give each new violation a line in `crates/lamina-core/tests/golden/triage.txt`: key, `true` or `false`, reason. Correct the example boards for true findings. If fewer than 7 of 10 violations of a rule are true, the rule defaults to `ignore`.
- [ ] Add one sentence to `docs/guide.md`. Commit.

### Phase 1: one check engine

No new checks. The golden files of Task 1.2 change only where a task says so.

#### Task 1.1: plan and progress files

- Create: `docs/specs/checks-plan.md` (a copy of this plan), `docs/specs/checks-progress.md` (the constraints, then one section for each task, as in `docs/specs/kicad-parity-progress.md`).

#### Task 1.2: golden record of the current results

- Create: `crates/lamina-core/tests/golden_checks.rs`, `crates/lamina-core/tests/golden/{tit,yoink}{,-tight}.txt`, `crates/lamina-core/tests/golden/triage.txt`. `lamina-core` sees the model checks and the DRC today.
- One sorted line for each violation: severity, rule, layer, items, message, position (0.001 mm). Then the excluded violations, the ratsnest, and the island count for each net.
- `fn tight(d: &mut ProjectData)`: clear the exclusions; set `clearance` 0.3, `min_track_width` 0.3, `min_annular_ring` 0.3, `hole_to_hole` 1.0, `copper_to_edge` 1.0, `silk_to_pad` 0.3, `silk_to_edge` 0.5, `min_silk_text_height` 1.5, `min_mask_web` 0.2, `min_thermal_spokes` 4.
- Tests: `golden_tit`, `golden_yoink`, `analysis_is_deterministic` (two runs give the same text), `each_golden_line_has_a_triage_line`. `golden_tit_tight` and `golden_yoink_tight` have `#[ignore]`, because they are slow in a debug build.
- With `LAMINA_BLESS=1`, a test writes its file.
- Verify: `cargo test -p lamina-core --test golden_checks`, then the same with `-- --ignored`. Each tight file must have more than 100 lines.

#### Task 1.3: numbers in a violation (model commit)

- Modify `crates/lamina-model/src/files.rs:726-785`: `actual: Option<f64>`, `required: Option<f64>` (serde default, skipped if none), `pub fn measured(self, actual: f64, required: f64) -> Self`.
- Test: `violation_measure_is_optional_and_not_in_the_key`.

#### Task 1.4: registry

- Create `crates/lamina-drc/src/registry.rs`:

```rust
pub enum Family { Model, Connectivity, Copper, Holes, Edge, Place, Zones, Mask, Paste, Silk, Labels, Assembly, Footprint, Electrical, Import }
pub struct CheckInfo {
    pub id: &'static str,
    pub family: Family,
    pub default: RuleSeverity,          // error, warning or ignore
    pub release: Option<RuleSeverity>,  // a higher severity for export
    pub overridable: bool,              // the user can change the severity
    pub excludable: bool,               // the user can exclude a violation
    pub summary: &'static str,          // one sentence: what the check reports
    pub why: &'static str,              // what goes wrong at the fab or on the bench
    pub fields: &'static [&'static str],// the Rules fields that it reads
}
pub const CHECKS: &[CheckInfo];              // 31 rows now; rows use ..BASE
pub const RULE_NAMES: [&str; CHECKS.len()];  // made from CHECKS
pub fn get(id: &str) -> Option<&'static CheckInfo>;
pub(crate) fn violation(id: &'static str, message: impl Into<String>) -> Violation;
pub fn check_rules(rules: &Rules) -> Result<(), String>; // unknown names in severities, blank exclusion reasons
```

- `check_rules` replaces the checks in `crates/lamina-core/src/ops/board.rs:229-236`. Call it also in `crates/lamina-build/src/lib.rs:407-440`, because a Jsonnet board has no such check today.
- Tests in `crates/lamina-drc/tests/registry.rs`: `ids_are_unique_and_old_ids_stay` (the 31 ids of today), `fields_are_rules_fields`.
- Verify: golden files do not change.

#### Task 1.5: split `analyze`

- **a.** Create `crates/lamina-drc/src/ctx.rs` (`Ctx::new` owns the pair scan and keeps the roots and the near pairs) and `finish.rs` (exclusions and severities, from `lib.rs:815-856`). Add `pub struct Connectivity { ratsnest, fills, geometry, island, fill_island }` and `pub fn connectivity(d: &ProjectData) -> Connectivity`, which runs no checks. Test: `connectivity_gives_the_islands_and_ratsnest_of_analyze`.
- **b.** Create the family modules `copper.rs` (clearance, shorts), `connect.rs` (unrouted, dangling items), `sizes.rs` (track width, drills, rings), `holes.rs`, `edge.rs`, `place.rs` (courtyards, keep-outs, unplaced), `zones.rs` (thermals, isolated copper), `mask.rs` (from `silk.rs`). `silk.rs`, `pairs.rs` and `lengths.rs` stay. All violations go through `registry::violation`. Each rule that compares a number with a limit gives `.measured()`.
- **c.** These callers need only connectivity; move them to `connectivity()`: `crates/lamina-route/src/lib.rs:1367, 1506`, `crates/lamina-core/src/ops/route.rs:535`, `crates/lamina-viewer/src/edits.rs:1237`. `export` runs the analysis one time, not two (`crates/lamina-core/src/ops/output.rs:58, 83`).
- Replace the two slow loops: the pad lookup for the ratsnest (`lib.rs:359-361`) with a map; the courtyard pairs (`lib.rs:716-733`) with `Index`.
- Verify: the golden files do not change (also the tight files). Record the time of each family in the progress file.

#### Task 1.6: one pipeline for all rules

- Move the checks of `crates/lamina-core/src/validate.rs` to `crates/lamina-drc/src/model.rs` (family `Model`, 11 rows). The analysis runs this family first, because `crates/lamina-core/tests/ops.rs:159` reads `new_violations[0]`.
- `overridable: false` and `excludable: false` for `bad_outline`, `missing_footprint`, `placement_unknown_part`, `pad_in_two_nets`, `unknown_pad`, `layer_not_in_stackup`, `unknown_net`.
- Give each model violation a position (`.at()`), so that it has a marker.
- If `Contour::check` fails, the pour and the edge checks treat the outline as absent.
- `Project::check_data` (`crates/lamina-core/src/project.rs:223-230`) calls only the analysis. Delete `lamina_drc::check`, `Checker`, `add_checker` and `register_checkers` (`crates/lamina-mcp/src/main.rs:198`); no code uses them.
- Remove the separate call in `crates/lamina-viewer/src/live.rs:156-160`. The gates at `ops/exclusions.rs:27` and `crates/lamina-viewer/src/main.rs:1072-1073` use `overridable` and `excludable`.
- Remove `pin_role` from `OTHER_WORDS` (`crates/lamina-mcp/src/doc_check.rs:39-40`) in the same commit. The test `other_words_are_needed` fails if you do not.
- Tests: `analyze_reports_model_errors_and_does_not_panic` (one board for each model rule, an outline with NaN, a track on `in5` of a 2-layer board), `pin_role_severity_can_be_set_and_unknown_net_cannot`, `model_error_has_a_marker`.
- Verify: `cargo test --workspace -j 2`. The golden diff shows only the added positions.

#### Task 1.7: strict mode counts violations (H13)

- Modify `Project::commit` (`crates/lamina-core/src/project.rs:262-268`): compare the count of each key, not only the set of keys.
- Test: `second_free_end_of_one_track_is_a_new_violation`.

#### Task 1.8: tool `checks`

- An operation in `crates/lamina-core/src/ops/status.rs` and a read-only tool in `crates/lamina-mcp/src/server.rs` (`lamina-mcp` has no direct dependency on `lamina-drc`): each rule with family, default, release severity, summary, reason and fields. Optional `family` filter.
- Change `README.md:41` to 75 tools and add `checks` to the Checks row (line 163).
- Test: `checks_lists_each_rule_with_its_default`.

#### Task 1.9: release view and exit codes (H3)

- Registry: `release: Some(Error)` for `unrouted` and `unplaced`.
- `Analysis` gets `release_only: Vec<Violation>`. `pub fn release_view(a: &Analysis, rules: &Rules) -> Vec<Violation>` in `lamina-drc`. A severity that the user set in `Rules.severities` wins.
- `export` (`crates/lamina-core/src/ops/output.rs:56-129`) refuses if the release view has an error and `force` is false. The export action of the viewer (`crates/lamina-viewer/src/actions.rs:324-337`) uses the same view.
- Tool `check` gets `release: bool`. `Status` gets `release_errors: usize`.
- CLI (`crates/lamina-mcp/src/main.rs:177, 411-415`): `lamina check <dir> [--release] [--fail-on error|warning|never] [--rules a,b]`. Exit code 0 is clean, 2 is violations, 1 is "cannot run". The default of `--fail-on` is `error`.
- Tests: `export_refuses_open_connections_unless_forced`, `user_severity_wins_over_release`, `ignored_rule_with_a_release_severity_shows_only_in_the_release_view`, `check_exit_codes` (put the decision in `fn exit_code(violations: &[Violation], fail_on: FailOn) -> i32`; the workspace has no CLI test crate, do not add one).

#### Task 1.10: `use_fab_preset` keeps the board data (H5)

- Add `pub fn apply(current: &Rules, name: &str) -> Option<Rules>` in `crates/lamina-drc/src/presets.rs`: the limits and the label come from the preset; `net_classes`, `custom`, `exclusions` and `severities` stay.
- Modify `crates/lamina-core/src/ops/board.rs:264-285` to use it.
- Test: `use_fab_preset_keeps_custom_rules_exclusions_and_severities`.

### Phase 2: copper and holes that no check sees

#### Task 2.1: copper text and copper drawings are copper (H1, H8)

- **a. Geometry.** Modify `crates/lamina-geom/src/board.rs`: add `Owner::Text { id }` and `Owner::Graphic { id }`. `build` adds board texts and board drawings on copper layers, and footprint drawings on `FpLayer::Copper` (their label is the refdes), as `CopperItem` with no net. Move `stroke`, `shape_prims` and `text_prims` from `crates/lamina-drc/src/silk.rs:74-82, 260-315` to `lamina-geom`. Use the stroke width of the scene (`font::silk_stroke_width`), so that the checked copper is the plotted copper.
- Decisions for each `match` on `Owner`:

| Place | Decision |
|---|---|
| `board.rs:36-48` `label` | the id |
| `board.rs:53-59` `body` | `text:{id}`, `graphic:{id}`: one body for each text |
| `lamina-drc` keep-out check (`lib.rs:760-766`) | `forbid.tracks` forbids it |
| `lamina-viewer/src/tools.rs:318-323` (magnet) | `continue` |
| `lamina-scene/src/lib.rs:383, 401` | remove the copper arms: `lib.rs:295-301` draws all `g.copper`. If the arms stay, the Gerber has the art two times |
| pours (`fill.rs:193-223`), router (`lamina-route/src/lib.rs:404-437`), edge check | no change: an item with no net is an obstacle |

- **b. Checks.** In the pair scan, skip a pair if the two items are texts or drawings with no net. A board from a Gerber import has only such drawings (`crates/lamina-export/src/fab_import.rs:285-306`); without this rule, each pair that touches is a short. A copper text or drawing with a stroke thinner than `min_track_width` gives `track_width`.
- Tests: `copper_text_round_trip` in `crates/lamina-export/tests/readback.rs` (the Gerber copper is the model copper; it fails today for a footprint drawing), `copper_text_reports_clearance_edge_and_keepout`, `touching_netless_graphics_are_quiet`, `pour_keeps_clear_of_copper_text`, `router_goes_around_copper_text`.

#### Task 2.2: `hole_to_copper` (H2)

- **a.** Model commit: `Rules.hole_to_copper: f64`, serde default 0.2. Presets: 0.2 (`jlcpcb-*`), 0.3 (`relaxed`). The value must not be above `min_annular_ring + clearance` of the preset, or a legal via next to a track fails.
- **b.** `DrillItem` (`crates/lamina-geom/src/board.rs:72-78`) gets `net: Option<String>` and `layers: (u8, u8)`. The router marks each hole as an obstacle for tracks of other nets (`crates/lamina-route/src/lib.rs:496-503`). A pour keeps `max(zone clearance, hole_to_copper)` from each hole of another net and from each non-plated hole (`crates/lamina-geom/src/fill.rs:224-233`). Tests: `router_keeps_tracks_out_of_a_non_plated_hole` (it fails today), `pour_keeps_hole_to_copper`.
- **c.** Check `hole_to_copper` (error) in `holes.rs`: a hole wall is nearer than the limit to copper. Skip the copper of the same part (the pegs of a USB-C socket are near its pads by design; Phase 5 examines the footprint), the copper of the same net for a plated hole, and pour pieces. Use only the layers of the hole. Tests: `track_through_a_non_plated_hole_is_an_error`, `track_at_the_limit_is_quiet`, `own_part_is_quiet`, `blind_via_hole_only_on_its_layers`.
- **d.** `crates/lamina-kicad/src/project.rs:155`: write `min_hole_clearance` from `hole_to_copper`, and read it on import.
- Verify: the golden diff has only `hole_to_copper` lines. Examine each line. Do not exclude a line automatically.

#### Task 2.3: vias that the fab output cannot make (H4)

- `crates/lamina-export`: the fabricator refuses a board that has a via that does not go through all copper layers: "The fabrication output cannot make blind or buried vias: v1, v2." `force` does not change this. Task 6.4 removes the refusal.
- Test: `fabricator_refuses_blind_vias`.

#### Task 2.4: bad outlines and bad layers stop nothing (H14)

- New model rule `zone_outline` (error, not overridable): the outline of a zone or of a keep-out crosses itself or has fewer than 3 points. The pour skips that zone.
- `autoroute` refuses a board that has a model error: "Correct these errors first: ...". A layer number above the stackup cannot reach `Grid::cover` (`crates/lamina-route/src/lib.rs:117-126`).
- Tests: `bow_tie_zone_is_reported_and_not_filled`, `autoroute_refuses_a_board_with_model_errors`.

#### Task 2.5: recall tests for the real defects

- Create `crates/lamina-core/tests/recall.rs`. Each test puts one real defect back into a copy of an example board and expects the rule. Tests for Phase 2: `copper_text_on_a_track_is_found`, `track_through_a_mounting_hole_is_found`. Each later phase adds its tests here (Tasks 3.1, 3.2, 5.5, 8.2).

### Phase 3: import and export keep the board (track B)

#### Task 3.1: footprint keep-outs through KiCad (H6)

- Modify `crates/lamina-kicad/src/footprint.rs:254`: a `zone` with a `keepout` node in a footprint becomes an `FpKeepout`. Layers F.Cu only give `Front`, B.Cu only give `Back`, all other sets give `AllCopper`. Map the five flags to `Forbid`.
- Modify `crates/lamina-kicad/src/pcb.rs:997-1186`: write `fp.keepouts` as zones in the footprint. Modify `crates/lamina-specctra/src/lib.rs:156`: write part keep-outs with `lamina_geom::part_keepouts`.
- Tests in `crates/lamina-kicad/tests`: `footprint_keepout_survives_import` (fixture `ESP32-S3-WROOM-1`; expect 1 keep-out with `forbid.pours`), `footprint_keepout_round_trip`.
- Verify: import `C:\Users\mranderson\Desktop\tit\pcb\ucus_karti.kicad_pcb` into a temporary folder. The pour must not fill the antenna area, with no keep-out added by hand.

#### Task 3.2: paste and mask drawings reach the Gerber files (H7)

- Modify `crates/lamina-kicad/src/footprint.rs:560-563`: a pad with no copper on its side becomes a filled `FpGraphic` on `FpLayer::Paste` or `FpLayer::Mask` (or both) with the pad shape.
- Modify `crates/lamina-scene/src/lib.rs:241-248`: plot `FpLayer::Paste` and `FpLayer::Mask` drawings on `SceneLayer::Paste(side)` and `SceneLayer::Mask(side)`. Lines 397-409: do the same for board drawings on mask and paste layers.
- Tests: `paste_only_pads_become_paste_drawings` (fixture `ESP32-S3-WROOM-1`: 9 paste pads on the ground pad), `footprint_paste_drawing_is_in_the_paste_gerber`, `footprint_mask_drawing_opens_the_mask`.
- Verify: a new import of the T.I.T board has 9 paste openings on U1.41. Then correct `examples/tit-flight-computer`, which has none today.

#### Task 3.3: KiCad import corrections (H9)

One commit and one test for each line.

- `pad_to_mask_clearance` 0 gives `mask_margin` 0 (`pcb.rs:62-69`).
- A pad with `*.Mask` and no paste layer has `paste: false` (`footprint.rs:475-487`).
- `.kicad_pro` (`project.rs:13-120`): read `min_clearance` (the board clearance is the larger of it and the Default class value), `min_hole_clearance`, `min_text_height`, `min_resolved_spokes`.
- A closed loop of footprint drawings on Edge.Cuts becomes a board cut-out at the part position (`layers.rs:74`, `footprint.rs:223`).
- A pad property `fiducial` or `testpoint` sets `Part.fields["kind"]`.
- These give a loss record (Task 3.5), not silence: a zone with `thru_hole_only`, a pad with a drill offset, a `.kicad_dru` file, a hatch pour, a second polygon of a zone, a custom pad with more than one primitive, a text box.

#### Task 3.4: KiCad export corrections (H11)

- `crates/lamina-kicad/src/pcb.rs:1020-1083`: the Reference is visible only if the footprint has a `${ref}` text on the Silk layer. Write a `${ref}` text on Fab as a Fab user text.
- New companion file `<board>.kicad_dru` (through `BoardFormat::companions`): the base limits and then one KiCad rule for each `CustomRule`, in the same order. `net` gives `A.NetName == '<pattern>'`, `class` gives `A.NetClass == '<class>'`, `layer` gives `(layer "<name>")`. In `.kicad_pro`, write each board minimum as the smallest value that any rule permits, so that the minimum of KiCad does not hide a custom rule. Example of the syntax: `C:\Users\mranderson\Desktop\tit\pcb\ucus_karti.kicad_dru`.
- Tests: `reference_is_hidden_when_only_fab_has_it`, `custom_rules_are_in_the_dru_file`.
- Verify with KiCad (section 4): a copy of the collar board with the custom rule "via-ring" gives no `annular_width` error.

#### Task 3.5: losses stay visible (H10)

- Modify `crates/lamina-ports/src/format.rs:27-36`: `ImportedBoard` gets `losses: Vec<Loss>` with `Loss { kind: LossKind, what: String, count: usize, refdes: Option<String>, at: Option<Point> }`. `LossKind` says if the loss changes the fab data: `Keepout`, `PastePad`, `CopperDrawing`, `PadPrimitive`, `SkippedPad`, `SkippedTrack`, `ZoneConnection`, `EdgeDrawing` do; `Other` does not. Each adapter fills it for each construct that it drops (KiCad, EasyEDA).
- Modify `crates/lamina-core/src/ops/format.rs:176-178, 261-264`: each loss becomes an open `Note` in `notes.json` with `author: "import"`. A second import of the same file does not add the same note again.
- New rule `import_loss` (warning; error for release): one violation for each open import note of a kind that changes the fab data; `items` is the note id. `resolve_notes` removes it.
- `BoardExported` (`ops/format.rs:304-311`) gets `warnings: Vec<String>` for the data that the format cannot hold.
- Tests: `import_losses_become_open_notes`, `import_loss_goes_when_the_note_is_resolved`, `second_import_adds_no_duplicate_notes`.

### Phase 4: KiCad parity

| Rule id | KiCad item | Reports | Limit (default) | Severity |
|---|---|---|---|---|
| `drill_size` (extend) | drill_out_of_range | Also board holes; a drill above `max_drill`; a slot narrower than `min_slot_width` | 6.3; 0.5 | error |
| `annular_ring` (extend) | annular_width, padstack | Also plated board holes | `min_annular_ring` | error |
| `hole_to_hole` (extend) | holes_co_located | The message says "the holes overlap" at 0 mm | `hole_to_hole` | error |
| `via_count` | too_many_vias | A net has more vias than `Net.max_vias` | per net | warning |
| `courtyard_missing` | missing_courtyard | A part with pads has no courtyard drawing | none | warning |
| `courtyard_open` | malformed_courtyard | Courtyard lines do not close a loop | none | warning |
| `courtyard_overlap` (extend) | courtyard_clearance | Courtyards nearer than `courtyard_clearance` | 0 | error |
| `weak_connection` | connection_width | Two copper items of a net connect only where they touch, with an overlap narrower than `min_connection` | 0.05 | warning |
| `track_zero_length` | track_segment_length | A segment shorter than 0.01 mm | fixed | warning |
| `track_angle` | track_angle | Two tracks of a net join at less than `min_track_angle` (acid trap), outside pad, via and pour copper; no teardrop tracks | 90 degrees | ignore |
| `zone_overlap` | zones_intersect | Two zones of different nets and equal priority overlap on a layer | none | warning |
| `mask_bridge` (extend) | solder_mask_bridge | A mask opening shows a track or a via of another net; mask drawings count as openings | `min_mask_web` | warning |
| `mask_missing` | (Fusion, Cadence) | A pad that gets solder has no mask opening | none | warning |
| `silk_on_hole` | silk_over_copper | Silkscreen over a drilled hole that is not a via | `silk_to_pad` | warning |
| `silk_outside_board` | silk_edge_clearance | Silkscreen outside the outline or in a cut-out | none | warning |
| `silk_line_width` | (fab pages) | A silkscreen line thinner than `min_silk_width`; one violation for each footprint | 0.10 | warning |
| `text_variable` | unresolved_variable | A text has a `${...}` variable that Lamina does not know | none | warning |

- **Task 4.1** holes and rings (H15): `drill_size`, `annular_ring`, `hole_to_hole`.
- **Task 4.2** `via_count` (model: `Net.max_vias: Option<u32>`).
- **Task 4.3** courtyards (H12): `courtyard_missing`, `courtyard_open`, `courtyard_clearance`. `lamina_geom::courtyard` tells also if the courtyard is a drawing or a box from the pads, and how many lines stay open (`chain_edges` gives the count today, and `courtyard` discards it).
- **Task 4.4** copper: `weak_connection` (H17), `track_zero_length`, `zone_overlap`, `track_angle`. For `weak_connection`: build the connections of each net a second time with each item made smaller by half of `min_connection`. If a net has more islands in the second run, report the pairs that connect only in the first run.
- **Task 4.5** mask and silkscreen: `mask_bridge`, `mask_missing`, `silk_on_hole`, `silk_outside_board` (H16), `silk_line_width`, `text_variable`.
- **Task 4.6** KiCad rules in the two directions: read and write each `.kicad_pro` rule that has a Lamina field. Write `rule_severities` from `Rules.severities`. The map of rule names is in `lamina-kicad`.
- **Task 4.7** custom rules, second version. `CustomRule` gets `hole_to_copper`, `courtyard_clearance`, `max_track_width`, `min_track_angle`. `RuleMatch` gets `kind` (`track`, `via`, `pad`, `zone`, `hole`), `part` (refdes with `*` and `?`) and `area` (the id of a `RuleArea { id, layers, outline }` in `routing.json`; new tool `add_area`). Change `Rules::resolve` one time to take a subject struct (net, class, layer, kind, part, position). Update the fixed lists in `set_rules` (`ops/board.rs:193-198, 215-220`). Write the new forms to `.kicad_dru`. Note: `deny_unknown_fields` makes an old Lamina refuse a board with the new fields.
- **Task 4.8** two tests for the trust in pours (no runtime check): `fill_keeps_its_limits` (for each pour piece of the real boards and of three synthetic boards, the distance to copper of other nets, to holes and to the edge is not below the limit minus 0.001 mm); `fill_has_no_thin_necks` (an opening of each piece by the zone minimum width changes its area by less than 0.1 %).
- **Task 4.9** one ignored test in `crates/lamina-kicad/tests/kicad_cli.rs`, active when `KICAD_CLI` is set: export a fixture board that has one violation for each rule with a KiCad name, run `kicad-cli pcb drc`, and compare the two reports. KiCad fills the zones again, so keep a list of the differences that have an explanation.

### Phase 5: footprint evidence

This phase is the first new feature. It makes the datasheet check of a footprint a recorded step that you can repeat.

- **Task 5.1** model commit. `pub fn pads_hash(fp: &Footprint) -> String` in `lamina-model`: a stable hash of the pads (number, position, shape, drill) and the keep-outs at 1 µm. `Footprint.evidence: Vec<Evidence>` with `Evidence { pads_hash, method: package | land_pattern | second_source | preset | accepted, document, page, url, mpn, reason, inputs (the numbers that the check used), checked (date), result: pass | fail, findings: Vec<String> }`. `Footprint.source_hash: Option<String>` (the pads hash at import). Two footprints with the same pads hash share the evidence.
- **Task 5.2** `crates/lamina-footprint/src/compare.rs`: `pub fn align(a: &Footprint, b: &Footprint) -> Alignment` and `pub fn compare(a, b, opt: &CompareOptions) -> Comparison`. `align` tries 4 turns and the mirror, and finds the shift by the vote of the pads that agree. It runs one time by pad number and one time by pad geometry. If the geometry fits and the numbers do not, the result says "pads have different numbers", "turned by N degrees" or "mirrored (bottom view)". `fit` in `crates/lamina-core/src/reuse.rs:207-236` is not sufficient: it needs equal number sets, uses the centroid (one bad pad moves all results) and has no mirror. The comparison gives the turn, the origin shift, and for each pad the limits, the value and the difference; the pads that only one side has; the smallest hole margin.
- Tests: the same package from two sources passes; SOT-23 against SOT-323, 0603 against 0805 and pitch 2.50 against 2.54 fail; a mirror, new pad numbers, a turn, a shift, pitch + 0.05 mm and drill − 0.2 mm each give the correct finding.
- **Task 5.3** read-only tool `compare_footprints { a, b }`. An id is a footprint of the project or `provider:id`.
- **Task 5.4** tool `verify_footprint { footprint, document, page, url, mpn, package | pads | leads | accept }`:
  - `package`: a `PackageSpec` (`crates/lamina-footprint/src/build.rs:11-87`) with the datasheet dimensions. Exact items: pad count, pad numbers, pitch (0.02 mm, and 0.05 mm over a full row for SMD, 0.10 mm for through-hole). Land sizes: calculate `ipc::land` for the densities `Least` and `Most`; the toe, heel and width of each pad must be in that band, plus or minus 0.05 mm.
  - `pads`: the land pattern that the datasheet recommends. Centre 0.05 mm below a pitch of 0.65 mm, else 0.10 mm. Size −10 % to +25 %, larger only away from the part.
  - `leads`: for through-hole parts, the lead size for each pad. The hole must be the largest lead dimension (the diagonal for a square pin) plus 0.15 mm or more. Above plus 0.5 mm is a warning.
  - `accept`: a reason to trust the source with no comparison.
  - A footprint that was made from a land table needs `package` or a second source: the same table a second time proves nothing.
  - The tool stores the evidence in the footprint file and returns the table of differences and the items that it did not check.
- **Task 5.5** rules:

| Rule id | Reports | Severity |
|---|---|---|
| `footprint_stale` | The pads changed after the evidence was recorded (hash). It has priority over `footprint_mismatch`. | warning |
| `footprint_mismatch` | The last evidence of a footprint in use is a fail | error |
| `footprint_changed` | The pads differ from the imported source (`source_hash`), and no evidence covers the current pads | warning |
| `footprint_unverified` | A footprint in use has no evidence for its current pads. One violation for each footprint. | ignore; error for release |
| `footprint_symmetry` | All pads of a footprint have a mirror partner, but some are 0.02 to 0.2 mm away from the mirror position | warning |
| `pin1_mark` | The pad pattern is the same after a turn, the pad numbers are not, and the silkscreen is also the same after that turn (no mark); or the mark is nearer to the turned position of pad 1 (error) | warning |

- `footprint_symmetry` needs no datasheet. On the T.I.T board it finds the USB-C socket: pads A1/B12 are at −3.15 mm and A12/B1 at +3.25 mm. The cause is in the script of that board (`C:\Users\mranderson\Desktop\tit\pcb\ucus_karti.py:320`): it moves the four pads on X, and its comment says "away from the peg", which is Y.
- Recall tests: a pad moved by 0.1 mm gives `footprint_stale`; the SOT-323 of the collar review fails `verify_footprint` on the pitch.
- **Task 5.6** tool `audit_footprints { online: bool }`, for each footprint in use with no evidence:
  1. If its package name is a standard package (`presets::resolve`), verify it against the preset (`method: preset`).
  2. Compare it with each other source of the same part in the project cache or the part library. Compare pad edges, not centres: inner gap 0.3 mm, outer span 0.5 mm, width 0.25 mm. A match stores `second_source`.
  3. With `online: true`, get the missing sources from the providers.
  - The result lists the differences for each footprint. A turn against the library of the assembler is a pick-and-place finding (Task 7.5).
  - `footprint_from_package` records `package` evidence with its inputs when it makes a footprint.
- **Task 5.7** `Status` (`crates/lamina-core/src/ops/status.rs:11-51`) gets `footprint_evidence: { verified, stale, mismatch, unverified }`. `docs/guide.md` gets a "Verify a footprint" section. The MCP `INSTRUCTIONS` (`crates/lamina-mcp/src/server.rs:17-39`) tell the agent to verify each new footprint.
- Verify: use a copy of the collar board with the footprints of the first commit. Give `verify_footprint` the datasheet numbers of J1 (Samtec SHF), BT1 (Saft LS 14500 2PF) and D1 (SOT-323). It must report the defects that the review found by hand.

### Phase 6: fab profiles and fab checks

- **Task 6.1** profiles as data. `crates/lamina-drc/src/fab/mod.rs` and `crates/lamina-drc/src/fab/profiles/{jlcpcb,pcbway,relaxed}.json`, read with `include_str!`. A workspace `lib/fabs/*.json` adds user profiles. `FabProfile { id, name, source (URL), checked (date), tiers: Vec<FabTier> }`, `FabTier { id, layers: (min, max), copper_oz, limits: FabLimits }`. `FabLimits` has the limit fields of `Rules` and the fields of Task 6.3. Get the values from the capability pages on the day of the task. Do not copy the numbers of the current presets. Set `min_mask_web` (0.1 for JLCPCB): it is 0 in each preset today, so the mask checks are silent. The old preset names stay as aliases.
- **Task 6.2** read-only tool `board_minimums`: the smallest features of the board, each with its value, its items and its position: copper distance between nets, track width, via drill, via diameter, ring, drill, slot, hole to hole, hole to copper, copper to edge, hole to edge, mask web, silkscreen line, text height, drill aspect, board size. One scan with a radius of 0.5 mm; a larger value shows as "0.5 or more".
- Read-only tools `fab_profiles` (the profiles with source and date) and `fab_compat { profiles }`: compare the minimums with each profile. For each profile: `fits`, and each limit with value, limit and margin, the worst first. No second analysis.
- **Task 6.3** new fab checks:

| Rule id | Reports | Limit (default) | Severity |
|---|---|---|---|
| `hole_to_edge` | A hole wall nearer than `hole_to_edge` to the board edge | 0.3 | error |
| `drill_aspect` | Board thickness divided by the smallest plated drill above `max_drill_aspect`; one violation for the board | 8 | warning |
| `mask_sliver` | Mask between two openings thinner than `min_mask_web` and not zero | `min_mask_web` | warning |
| `board_neck` | A part of the board, or a slot, narrower than `min_route_width` | 0.8 | warning |
| `hole_to_copper` (extend) | Different limits for via holes, plated holes and non-plated holes | per profile | error |
| `hole_to_hole` (extend) | A different limit between via holes | per profile | error |

- **Task 6.4** fab output: one drill file for each via span (this removes the refusal of Task 2.3); Gerber X2 aperture, net and component attributes; a `.gbrjob` file.

### Phase 7: assembly and BOM

| Rule id | Reports | Limit (default) | Severity |
|---|---|---|---|
| `fiducial_count` | A side with SMD parts has fewer fiducials than `min_fiducials` | 0 = off | warning |
| `part_to_edge` | A part body nearer than `part_to_edge` to the board edge | 0 = off | warning |
| `part_height` | A part higher than the `max_height` of a keep-out that it overlaps | per keep-out | error |
| `paste_missing` | A footprint has SMD pads with paste and SMD pads without paste, and no paste drawing covers the pads without paste (true on T.I.T U1.41) | none | warning |
| `paste_oversize` | A paste opening larger than the pad copper | none | warning |
| `tombstone` | A small two-pad part with one pad on wide copper and one on a thin track | fixed | warning |
| `via_in_pad` | A via in an SMD pad, but not in a pad of its net that is wider than 3 via diameters | none | warning |
| `testpoint_access` | A test point is under a part, has no mask opening, is smaller than `min_testpoint`, or is nearer than `testpoint_to_edge` to the edge | 1.0; 0 = off | warning |
| `bom_part_number` | A BOM line (value and footprint) has no MPN and no LCSC number; parts with `exclude_from_bom` and DNP parts do not count | none | ignore; warning for release |
| `bom_conflict` | One MPN with two footprints (by pads hash); for R, C, L, D, Q and U parts also one MPN with two values, or one value and footprint with two MPNs | none | warning |

- A fiducial is a part with `Part.fields["kind"] = "fiducial"`, or with one SMD pad with no net and no paste and a reference that starts with `FID`. A test point has `kind = "testpoint"`, or one pad and a reference that starts with `TP`.
- `tombstone`: two equal SMD pads with paste, centres 1.7 mm or less apart. The escape width of a pad is the sum of the widths of its tracks (no teardrops) plus spokes × spoke width (`ZoneFill.pad_thermals`); a solid pour counts as the pad perimeter. A pad is "wide" if it is in a solid pour, or if its escape is 0.8 of its short side or more. Warn if one pad is wide and the ratio of the two escapes is 3 or more.
- **Task 7.1** placement checks: `fiducial_count`, `part_to_edge`, `part_height` (model: `Keepout.max_height: Option<f64>`; the height is `Footprint.height`), `testpoint_access`.
- **Task 7.2** paste checks. `export` gets `paste_for_dnp: bool` (default false: no paste for DNP parts). A stencil area ratio check is later work.
- **Task 7.3** `tombstone`, `via_in_pad`.
- **Task 7.4** BOM (H18): the two rules; columns `Manufacturer` and `MPN`; `exclude_from_bom` removes a part from the BOM (`crates/lamina-export/src/lib.rs:153-183`).
- **Task 7.5** CPL (H18, `crates/lamina-export/src/lib.rs:184-209`): "Mid" is the centre of the pads, not the footprint origin; `Part.fields["cpl_rotation"]` adds a correction; `audit_footprints` proposes it when a footprint is turned against the library of the assembler. IPC-D-356: a refdes longer than 6 characters gets an alias and does not stop the export.

### Phase 8: label checks and fixes

Rules for the three label checks: the font has one width for all characters, so the centre of each word comes from its character positions, the text turn and the mirror of the bottom side. Distances go from the text box to the courtyard. A connector is a part with a reference that starts with J, P, CN, X, TB or TP. A through-hole part takes labels from the two sides.

| Rule id | Reports | Severity |
|---|---|---|
| `silk_under_part` | A board text, or the reference of another part, is in the body outline of a part on the same side | warning |
| `label_far` | The first word of a text (or a footprint `${ref}` text) is the reference of a part, and: the text is more than `label_max_distance` (5 mm) from that part, or it is in the courtyard of a different part, or it is more than 2 mm away and a different part is nearer by 1.5 mm | warning |
| `label_pin` | Words of a text are net names of the pads of a connector within 2.5 mm, and they are in the wrong position (steps below) | warning |
| `polarity_mark` | A `+` or `-` mark is nearest to a pad of the opposite polarity, and the correct pad is more than 0.25 pitch farther | warning |

- `label_pin` steps:
  1. Drop numbers, words of one character, and a word that is the value of the nearest part.
  2. A legend ("1 M+ 2 GND"): compare each name with the net of the pad of that number. Do no position test.
  3. A word matches a pad if it is the net name, or if the net ends in `_WORD`. VCC, VDD and VIN match the only power net of the connector. GND and VSS match its ground net. Never match TX/RX or MISO/MOSI.
  4. Two or more matched words along the pad row (15 degrees): project the words and the pads on the row axis. If all pairs are in the reverse order, give one warning: "the labels are in the reverse order of the pins".
  5. One matched word: warn if the nearest pad has a different net and the correct pad is more than 0.5 pitch farther.
- `polarity_mark`: a mark is a word `+` or `-`, or a name that ends in one (IN+, OUT-). A `-` alone needs a `+` in the same text or at the same part. A pad is plus if its net has a power role or ends in `+` or `_P`; minus if its net has a ground role or ends in `-` or `_N`.
- **Task 8.1** `silk_under_part`.
- **Task 8.2** `label_far`, `label_pin`, `polarity_mark` in `crates/lamina-drc/src/labels.rs`. Recall tests (Task 2.5 file): the collar label "U1 nRF54L15" 10 mm from U1; the Yoink label "PCA9685 GND OE SCL SDA VCC" turned 90 degrees at J11 (pin nets GND, GND, SCL, SDA, 3V3); "VBAT + -" centred above a two-screw terminal. Each one must warn. The two example boards must give no warning: the Yoink legend "XH: 1 M+ 2 GND ...", the T.I.T labels "BOOT", "ARM", "PYRO1" and "1" to "4" are the known traps.
- **Task 8.3** model commit: `Violation.fix: Vec<Fix>`, `enum Fix { MoveText { id, to }, TurnText { id, by }, SetTextSize { id, size }, SetViaSize { id, diameter, drill }, SetTrackWidth { id, width } }`. The silkscreen and label rules, `annular_ring` and `track_width` give a fix when a safe one exists.
- **Task 8.4** tool `apply_fixes { rules, items, dry_run }`: apply each fix as a strict change; return the applied and the refused fixes with the reason.

### Phase 9: review, CI formats and diff

- **Task 9.1** tool and CLI `review`: a verdict (`ready`, `ready_with_warnings`, `not_ready`) and sections: release check by family, fab compatibility, footprint evidence, BOM, open import losses, exclusions with reasons, open notes, parts with no pin names. It writes `output/review.md`. With a renderer port, it adds images with markers. Update the MCP `INSTRUCTIONS`: run `review` before `export`.
- **Task 9.2** `lamina check --format text|json|sarif|junit`; an example GitHub Actions workflow in `docs/guide.md`.
- **Task 9.3** `lamina diff <a> <b>`: parts added, removed and moved; nets and rules changed; violations added and fixed (by `Violation::key`).

### Phase 10: electrical checks without a schematic

| Rule id | Reports | Data | Severity |
|---|---|---|---|
| `net_single_pad` | A net has one pad | none | warning |
| `pad_no_net` | A part has numbered pads in no net; one violation for each part | none | ignore |
| `track_current` | A track is too narrow for the current of its net (IPC-2221, `max_temp_rise` 10 °C, copper weight of the stackup) | `Net.current` | warning |
| `voltage_clearance` | Copper of two nets is nearer than the IPC-2221 table needs for their voltage difference | `Net.voltage` | error |
| `decoupling_far` | A power pin of a part (by its pin name) has no capacitor pad of its net within the larger of 3 mm and 2 pin pitches; one warning for each part and net | pin names | warning |
| `return_path` | A track of a differential pair, of a net with a length limit, or of a class with `reference_plane` goes over a gap in the pour of the next layer | nets | warning |

- **Task 10.1** model commit: `Net.current`, `Net.voltage`, `NetClass.reference_plane`; `set_nets` and `lamina.libsonnet` accept them.
- `voltage_clearance`: put the table value into `Rules::resolve`, so that the pour and `clearance` use the same number.
- `decoupling_far` needs pin names. The two example boards have none (0 of 212 pads), because they come from KiCad boards with no schematic. A part with no pin names gets no check. Do not use net names as a replacement: a sense pin on a power net gives a false alarm.
- The IPC-2221 table is a JSON file with its source. Compare the numbers with the standard before you use them: I did not verify the values in my research notes.

## 4. Verification

- For each commit: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace -j 2`.
- Golden: `cargo test -p lamina-core --test golden_checks` and the same with `-- --ignored`. A changed file needs a triage line for each new violation.
- Recall: `cargo test -p lamina-core --test recall`. Each real defect of sections 1.2 and Context has a test that puts it back and expects the rule.
- Speed: `cargo run --release -p lamina-drc --example timing -- examples/tit-flight-computer`. Limit: 0.5 s.
- Real boards after each new check: `lamina check examples/tit-flight-computer`, `lamina check examples/yoink-main-board`, and a copy of the collar board. The two examples are KiCad imports with 2 layers, parts on the top only and no pin names. For rules that need more, build a board in the test code: 4 layers, parts on the two sides, pin names, one blind via.
- At the end of each phase, with a network: `LAMINA=target/release/lamina python3 examples/c3-sensor/test_build.py -v`. That test expects zero violations (`test_build.py:272-273`), so a new check can break it. Correct the example or the check.
- MCP run: start `lamina mcp` on a temporary copy of a board, call the new tool, read the result.
- Viewer: `lamina-viewer shot <board> --out x.png` shows the markers of a new rule. No window.
- KiCad second opinion (optional; KiCad 10 is at `C:\Users\mranderson\AppData\Local\Programs\KiCad\10.0\bin\kicad-cli.exe`): `lamina export-board --format kicad --out <tmp>/b.kicad_pcb <copy>`, then `kicad-cli pcb drc --format json --severity-all --output d.json <tmp>/b.kicad_pcb`.

## 5. Later work (not in this plan)

- A netlist import (KiCad netlist) with a parity check; ERC with pin types.
- A `cross_check` tool that runs the KiCad check from Lamina (see D6).
- Microvias, back-drill, pads with a drill offset, zone connection for each pad, hatch pours, a net on a copper drawing (antennas, shields).
- Track arcs; impedance with dielectric data; length in time units; creepage along the surface with slots.
- Panels, V-cut rules, IPC-2581 and ODB++.
- 3D collision checks with the part meshes and a housing STEP file.
- A distance table between part classes; test point coverage of the nets; a stencil area ratio check.
- Part data checks (capacitor voltage, resistor power) and BOM lifecycle data from the network.

## How to run this plan

Run one phase in each session, Phase 1 first. Use a new agent for each task and a review after each task (subagent-driven). The tasks of a phase share interfaces, and a check that is wrong can pass a bad board.
