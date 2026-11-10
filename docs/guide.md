# Lamina user guide

This guide has the details of the tools, the viewer and the file formats. To
start, read the [README](../README.md). For the crates and the design, read
[architecture.md](architecture.md).

- [Agent tools](#agent-tools)
- [Viewer](#viewer)
- [Autorouter](#autorouter)
- [Modular boards](#modular-boards)
- [Copy the layout of a block](#copy-the-layout-of-a-block)
- [Providers](#providers)
- [Part library and offline work](#part-library-and-offline-work)
- [KiCad files](#kicad-files)
- [Board formats](#board-formats)
- [EasyEDA boards](#easyeda-boards)
- [Gerber files](#gerber-files)
- [STEP export](#step-export)
- [Other autorouters](#other-autorouters)

## Agent tools

An agent usually calls the tools in this order:

1. `open` with `create: true`
2. `use_fab_preset`
3. `set_outline`
4. `import_part` for each part
5. `set_nets`, then `pins` to compare each pin name with its net
6. `place_many`
7. `autoroute`
8. `add_zone` for the ground pour
9. `check`
10. `render`
11. `export`

The `notes` tool gives the agent the notes that you type in the viewer.

A part from a provider carries the pin names of its datasheet. The viewer draws the pad
number and the pin name on each pad when you zoom in, and the inspector shows them when
you hover a pad. `check` warns when a ground pin is on a power net, a power pin is on a
ground net, or a no-connect pin is on a net (`pin_role`). `pins` lists every pad of a part
with its name and its net.

`check` also warns when silkscreen text or a silkscreen drawing is nearer to a pad than
the `silk_to_pad` rule (`silk_to_pad`).
It also checks overlapping silk (`silk_overlap`), silk near the board edge
(`silk_to_edge`), and small silk text (`silk_text_size`). Set the limits with
`silk_to_edge` and `min_silk_text_height`. `min_mask_web` sets the minimum gap
between pad mask openings (`mask_bridge`). Copper checks report free track ends
(`track_dangling`), vias with no pad connection (`via_dangling`), and zone pieces
with no pad connection (`isolated_copper`).

Agents can set the severity of a DRC check with `set_rules`, for example
`{"rules":{"severities":{"unrouted":"ignore","silk_overlap":"error"}}}`.
Values are `error`, `warning`, and `ignore`. The map replaces the previous map;
an empty map restores the defaults. Unknown check names are refused with a list
of valid names. Ignoring `unrouted` hides its warnings but keeps the ratsnest.

`set_rules` also accepts `custom`, an ordered JSON list, for example
`{"rules":{"custom":[{"name":"bus clearance","when":{"net":"BUS*","layer":"top"},"clearance":0.4}]}}`.
Selectors are optional `net` (`*` and `?` wildcards), `class`, and `layer`.
Rules can set `clearance`, `min_track_width`, `min_via_drill`, and
`min_annular_ring` in mm. Later matches override the fields they supply; a
clearance between two nets uses the larger result. The list replaces the old
list; `[]` clears it. Checks, pours and routing use the resolved limits.

`net_lengths` returns total track length in mm and via count for each net.
Lengths include branches and exclude via depth; they are not pad-to-pad paths.
Use `set_nets` to set optional `length_min`, `length_max`, `skew_group` and
`max_skew` on a net. DRC warns with `length_out_of_range` or
`skew_out_of_range`. A group's smallest supplied `max_skew` limits the spread
between its longest and shortest total track lengths.

`set_diff_pairs` replaces the named pairs in `nets.json`, for example
`{"pairs":{"usb":{"positive":"D+","negative":"D-","gap":0.2,"width":0.25}}}`.
Each net may belong to one pair. `diff_pair_gap` checks parallel copper with a
0.01 mm tolerance; `diff_pair_uncoupled` checks same-layer parallel coverage,
allowing one centre spacing at segment ends for corners. These are layout
checks, not an impedance calculation. Modular sources can supply `diff_pairs`
beside `nets`; both are generated into `nets.json`.

`route_diff_pair` takes `pair`, `layer`, and a `points` centre path. Positive
goes left and negative right, each offset by half the gap plus half the width.
Both tracks are committed together; new DRC errors refuse the operation.
Sharp reversals and bends too short for the inner track are refused.

`tune_length` takes `net`, `track`, a zero-based `segment`, a total-net `target`,
maximum `amplitude`, and leg `spacing`, all lengths in mm. It inserts meanders
to the left of the selected segment and preserves its endpoints. Spacing must
allow the track width and clearance; new DRC errors refuse the change.

`add_teardrops` and `remove_teardrops` expose the same copper-track teardrops as
the viewer. Generated tracks have `tdrop` IDs and retain their net; Gerber
exports include them. The agent operations refuse new DRC errors.

`board_stats` reports dimensions, outline area minus cutouts, part counts by
side, placed pad counts, vias, drill sizes, smallest track and total track
length. Lengths are mm and area is square mm; drill bores are not subtracted.
Fabrication exports include an IPC-D-356A `.d356` electrical netlist in the
board-house zip. It uses decimils and unique aliases for net names. Pad names
longer than 4 characters, such as `A1B12` on a USB-C receptacle, do not fit the
pin field. They get aliases `P001`, `P002` and so on, in sorted order for each
part. An alias skips any name that the part already uses. Two parts can use the
same aliases, because each record also has the reference. A `C  PIN` comment
line maps each alias to its pad name, for example `C  PIN   J1-P001 A1B12`.
Values that cannot fit its fixed-width fields are refused instead of silently
truncated. A part can have at most 999 aliased pad names.

Use `exclude_violation` with a `rule`, the `items` from `check`, and a nonempty
`reason` to accept a specific violation. The match uses item IDs, so it stays
after an item moves. `exclusions` lists the saved reasons and hidden violations.
`include_violation` removes the entry. `stale_exclusion` reports entries that no
longer match. The viewer has the same actions in Checks and an Exclusions filter.

`npth_inside_courtyard` warns when a non-plated hole overlaps another part's
courtyard on either side. `starved_thermal` reports a pad with fewer connections
to a thermal zone than `min_thermal_spokes` (default 2). It counts surviving pour
arms and same-net tracks from the pad into the fill. A pad with pours on more
than one layer or in more than one zone is good if all its spokes together reach
the minimum, or if a solid pour of its net covers it. It checks Lamina's fill;
another program that fills the zones again can report different counts.

`checks` lists all checks: what each one reports, why, its severity on this board
and the rule fields that set its limits. The checks for data that is not valid
(for example `unknown_net` and `zone_outline`) are always errors: you cannot
ignore or exclude them. `pin_role` and the other model checks accept `severities`
and exclusions.

`hole_to_copper` (default 0.2 mm) is the minimum distance from a hole wall to copper
of another net, and from a non-plated hole to all copper. The check has the same
name. Pours and the autorouter keep this distance. The copper of the part that has
the hole does not count.

Texts and drawings on a copper layer are copper with no net. `clearance`, `short`,
`keepout`, the edge checks, the pours and the autorouter see them. Two drawings
with no net can touch.

`check` with `release: true` shows the board as `export` sees it: `unrouted` and
`unplaced` are errors. `export` refuses a board with release errors if `force` is
not true, and `status` gives their number in `release_errors`. A severity that
you set for a rule wins. `export` writes one drill file for the holes that go
through the board, one for each span of blind or buried vias (for example
`-L1-L2.DRL`), one for the non-plated holes, and a `.gbrjob` job file. The copper
Gerber files have the net, the part and the pad of each object (Gerber X2). Parts that
are not placed (DNP) get no paste; `paste_for_dnp: true` changes that. In the
IPC-D-356 netlist, a reference longer than 6 characters gets an alias (`Z00001`)
and a `C  REF` comment line. On the command line,
`lamina check <board> --release` has exit code 2 if a violation is an error
(`--fail-on warning` and `--fail-on never` change this), so CI can use it.

### Checks from KiCad

These checks have the names and the limits that follow: `via_count` (the
`max_vias` of a net), `courtyard_missing`, `courtyard_open`, `weak_connection`
(`min_connection`), `track_zero_length`, `track_angle` (`min_track_angle`, off by
default), `zone_overlap`, `mask_missing`, `silk_on_hole`, `silk_outside_board`,
`silk_line_width` (`min_silk_width`) and `text_variable`. `drill_size` and
`annular_ring` also examine the holes of the board, `max_drill` and
`min_slot_width`.

### Custom rules

A custom rule in `custom` gives different limits to some items. Its `when` part
selects the items: `net` (with `*` and `?`), `class`, `layer`, `kind` (`track`,
`via`, `pad`, `zone` or `hole`), `part` (a reference, with `*` and `?`) and `area`
(the id of a rule area). All the conditions must be true, and a later rule wins.
The values are `clearance`, `min_track_width`, `max_track_width`,
`min_via_drill`, `min_annular_ring`, `hole_to_copper`, `courtyard_clearance` and
`min_track_angle`.

`add_area` makes a rule area: an outline, and layers if the area is not for all
layers. `delete_items` removes it. The checks use all the conditions. The pours
and the autorouter use only `net`, `class` and `layer`: a rule with `kind`,
`part` or `area` does not change what they do.

KiCad board export writes the custom rules to a `.kicad_dru` file next to the
board. A rule with `area` is not in that file, and a comment line says so.

### Verify a footprint

A wrong footprint passes each geometry check, so Lamina keeps evidence for each
footprint. `verify_footprint` compares a footprint with the numbers of the
datasheet and stores the result in the footprint file. Give one of: `package` (the
package dimensions), `pads` (the recommended land pattern), `leads` (the lead
sizes of a through-hole part) or `accept` (a reason to trust the source).
`audit_footprints` examines all footprints with no evidence against the standard
packages (in the three IPC densities) and against the footprint of the same part
from its provider, as a second source. A footprint that came with a KiCad board is
compared with the footprint of that name in the KiCad library, with no tolerance:
this finds a pad that someone moved on the board. `online: true` permits the
network for a second source. `compare_footprints` compares two footprints: the turn, the mirror, the
move and each pad that differs.

The rules are `footprint_mismatch` (the evidence is a fail), `footprint_stale`
(the pads changed after the evidence), `footprint_changed` (the pads differ from
the imported source), `footprint_symmetry` (a pad is 0.02 to 0.2 mm from its
mirror position) and `footprint_unverified`. A footprint with no evidence is an
error only for release. `status` gives the numbers in `footprint_evidence`.

### Fabricators

`board_minimums` gives the smallest features of the board with their items.
`fab_profiles` lists the fabricator limits that Lamina has, each with its source
page and the date of the check. `fab_compat` compares the board with each profile
and gives the margin of each limit, the worst first. `use_fab_preset` with a
profile id (`jlcpcb`, `pcbway`) puts the limits of that fab into the rules. The fab checks are
`hole_to_edge`, `drill_aspect` (`max_drill_aspect`), `mask_sliver`
(`min_mask_web`) and `board_neck` (`min_route_width`).

### Assembly and BOM

- `fiducial_count`: a side with SMD parts has fewer fiducials than `min_fiducials`
  (0 is off). A fiducial is a part with one pad and a reference that starts with
  `FID`, or a part with the field `kind` set to `fiducial`.
- `part_to_edge`: a part is nearer to the board edge than `part_to_edge` (0 is off).
- `part_height`: a part is higher than the limit of a keep-out that it overlaps.
  Set the limit in `height_limits`, a map from the keep-out id to a height in mm.
- `paste_missing`, `paste_oversize`, `via_in_pad`.
- `testpoint_access`: a test point is under a part, has no mask opening, is
  smaller than `min_testpoint` or is nearer to the edge than `testpoint_to_edge`.
- `bom_conflict`: one part number with two values or two footprints.
- `bom_part_number`: a BOM line has no part number. It shows only for release.
- `tombstone`: off by default. It compares the copper on the two pads of a small part.

The BOM has the columns Manufacturer and MPN. A part with the field
`exclude_from_bom` set to `yes` is not in the BOM and not in the CPL. The CPL
position is the middle of the pads. The field `cpl_rotation` adds a correction in
degrees for a part whose zero angle differs from that of the assembler.

### Labels

Four checks read what the silkscreen says. They guess, so they are warnings.
`label_far`: a text names a part but is far from it (`label_max_distance`) or
nearer to a different part. `label_pin`: the pin labels of a connector are in the
reverse order of its pins, or a label is at the wrong pin. `polarity_mark`: a plus
or minus mark is at the pad of the opposite polarity. `silk_under_part`: a text is
below the body of a part.

Some violations have a `fix`. `apply_fixes` applies the fixes: a larger text, a
wider track, a larger via pad, a text turned by 180 degrees. It applies a fix only
if the fix adds no error. Use `dry_run` to see the fixes first.

### Electrical checks

These checks need no schematic. They use net data.

- `net_single_pad`: a net has one pad. `pad_no_net` (off by default): a part has
  pads in no net.
- `track_current`: set `current` (A) on a net. The check uses the IPC-2221 formula
  with `max_temp_rise` and the copper weight of the stackup.
- `voltage_clearance`: set `voltage` (V) on a net. A ground net has 0 V. The
  spacing values come from a table of IPC-2221C that is data in the program. The
  table agrees with one secondary source and was not compared with the standard:
  do not use this check as the only check for a safety distance.
- `decoupling_far`: a power pin of a part has no capacitor of its net within
  `decoupling_max_distance`. The check needs pin names.
- `return_path`: a track of a differential pair, of a net with a length limit, or
  of a net class with `reference_plane` goes over a gap in the pour of the next
  layer.

### Review and CI

`review` gives one verdict for the board (`ready`, `ready_with_warnings` or
`not_ready`) with the evidence, and writes `output/review.md` with images of the
top side, the bottom side and the violations. Call it before
`export`. On the command line:

```
lamina review <board>
lamina check <board> --release --format sarif
lamina diff <board-before> <board-after>
```

`--format` accepts `text`, `json`, `sarif` and `junit`. `lamina review` has exit
code 2 if the board is not ready. `lamina diff` lists the parts, nets, rules and
violations that differ, and has exit code 2 if the second board adds an error.

KiCad board import keeps the keep-out zones of a footprint (for example below an
antenna) and the paste and mask openings that a footprint has as pads with no
copper. KiCad board export includes filled zones, a project rules file, a `.pretty`
footprint library, and an `fp-lib-table` with project-relative paths. Keep these
files together when you share the board. The library includes mounting holes.

## Viewer

```
lamina-viewer [--3d] [project folder or board file]...
```

- You can open more than one board. The boards show side by side in one window.
- `--3d` starts in the 3D view.
- The layer panel, the inspector, the status bar and the shortcuts apply to the board
  that you clicked last.
- If you give no arguments, the viewer opens the boards from the last time.

To open a board, click `+` in the top bar or push `Ctrl+O`. You can also drop a folder or a
file on the window. The viewer opens Lamina project folders and KiCad, EasyEDA and Gerber
files. It imports a file from a different program into a project in the local data folder.
If that file changes, the viewer imports it again.

Keys:

- Click to select a part. Drag to move it. Drag a box to select more than one item.
- `R` rotate, `F` flip, `L` lock, `M` move, `Del` delete. The arrow keys move the
  selection a small distance.
- `X` route a track. `V` adds a via and changes the layer. `/` changes the bend.
- `V` via, `Z` pour, `K` keep-out, `O` outline.
- `D` measure. The measurement snaps to the centres of pads and vias.
- `N` grid. `Ctrl+Z` undo, `Ctrl+Y` redo.
- `Ctrl+F` find a part, a value or a net.
- Right-drag or `Space`+drag to pan. Scroll to zoom. `1` 2D view, `2` 3D view.
- `F1` shows all the shortcuts.

The Board menu has these commands:

- Autoroute the full board, or only the nets of the selection. You can undo it in one step.
- Rip up the tracks.
- Export the fab files (Gerber, drill, BOM, CPL), a STEP assembly or a KiCad board.

The inspector shows the values of the selected item. You can change them there.

The viewer and an agent can change the same board at the same time. Before each change,
each of them reads the files again, so neither writes over a change of the other.

## Autorouter

The `autoroute` tool and the Board menu route all open connections, or only the nets that
you name. You do not need a different program.

- The router routes each net as a tree. A tree can start or stop on any copper of the net,
  also on a pour.
- At first, tracks of different nets can overlap. After each pass, the cost of a shared
  cell increases. The router then routes the overlapped nets again, until no nets overlap.
  This method is PathFinder.
- If a connection fails at the width of its net class, the router tries the minimum width.
  If it fails again, the router removes the tracks near it. Then it routes that area again
  on a grid that is two times finer.
- The router routes the nets that have a pour last. Then it fills the pours again and
  routes the connections that are still open.
- The router obeys keep-outs, hole-to-hole clearance, the width and clearance of each net
  class, and the layers that it must not use.

Test: two real KiCad boards, with all tracks removed and the pours kept. The router
connected 58 of 58 and 94 of 94 connections, in about 12 seconds for each board. The DRC
found 0 errors. One of the boards has a USB-C D+/D- crossover between 0.5 mm pads.

To do the same test on a different board:

```
cargo run --release -p lamina-route --example bench -- <project>
```

## Modular boards

You can write a board in Jsonnet (`design.jsonnet`) instead of JSON. Jsonnet is JSON with
variables, functions and `import`. You can write a circuit block one time and use it
many times:

```jsonnet
local lamina = import 'lamina.libsonnet';
local servo = import 'servo.libsonnet';
lamina.merge([
  { board: { name: 'demo', outline: lamina.rect(64, 52, 3), rules: { preset: 'jlcpcb-2layer' } } },
  servo.channel('J2', 'R8', { pwm: 'PWM0', out: 'SERVO1', vservo: 'VSERVO', gnd: 'GND' }),
  servo.channel('J3', 'R9', { pwm: 'PWM1', out: 'SERVO2', vservo: 'VSERVO', gnd: 'GND' }),
])
```

`servo.channel` is not part of Lamina. It is a function in the `lib` folder of the board.
You write functions like this for your own circuits.

`lamina build <board>` makes `board.json`, `parts.json` and `nets.json` from the source.
When the source changes, the MCP server and the viewer build the board again. Do not edit
these three files. Lamina refuses edits to them. You edit the placement and the routing
with the tools or in the viewer, as usual.

`lamina.libsonnet` is part of Lamina. It has these functions:

- `merge` joins blocks into one board. A refdes must be unique. Nets with the same name
  join their pads.
- `group` gives the parts of a block a group name.
- `net`, `pads`, `rect`, `hole` and `corner_holes`.

More than one board can use the same blocks in a workspace. `lamina workspace <dir>` makes
`lamina.json`, with the `boards` and `lib` folders. Each `lib` folder can also have shared
`footprints/` and `models/`. `lamina new <dir> --modular` makes a board from a template.

## Copy the layout of a block

Put each instance of a block in a group. Make the layout of one instance. Then copy that
layout to the other instances:

```jsonnet
lamina.group('servo1', servo.channel('J2', 'R8', { pwm: 'PWM0', out: 'SERVO1', vservo: 'VSERVO', gnd: 'GND' })),
lamina.group('servo2', servo.channel('J3', 'R9', { pwm: 'PWM1', out: 'SERVO2', vservo: 'VSERVO', gnd: 'GND' })),
```

```
lamina repeat-layout boards/servo --from servo1 --to servo2 --anchor J3
```

Lamina pairs the parts by footprint and nets, not by name. A footprint from a different
library also pairs if its pads are at the same positions. Different pad numbers or a
different origin are not a problem. The anchor part does not move. Lamina puts the other
parts of the block around it.

Lamina also copies these tracks and vias:

- All tracks and vias of a net that is fully in the block.
- Short tracks to a via near the parts, on nets that go out of the block.

Lamina does not change the tracks to other parts of the board. After the copy, look at
the ratsnest. You can also give a block as a list of refdes (`--from D1,R1`).

To keep a layout for other boards:

- `lamina save-layout <board> --from led1 --name led_indicator` writes the layout to
  `layouts/led_indicator.json` in the workspace library.
- `lamina place-layout <board> --name led_indicator --to led2` puts it on a block of
  any board in the workspace.

The MCP tools are `repeat_layout`, `save_layout_block` and `place_layout_block`. In the
viewer, select a part of a group. Then use *Select group*, *layout like* or *Copy layout
to all like it*.

## Providers

Parts, footprints and 3D models come from providers. A part is named `provider:id`. The
`providers` tool lists them with the id format of each. Lamina ships two:

- `lcsc`: real parts from LCSC / JLCPCB, with stock, price, the vendor footprint and the
  vendor 3D model. Example: `lcsc:C6186`.
- `kicad`: generic footprints and package models from the KiCad libraries on this
  computer, or from the official online libraries. Example:
  `kicad:LED_SMD:LED_0603_1608Metric`, or the path of a `.kicad_mod` file.

`search_parts` searches all providers or one. `import_part` and `import_parts` import by
name and can add the part. `set_model` with `provider:id` puts a provider model on any
footprint. A new provider is one type that implements `Provider` in `lamina-model`, plus
one line in `Providers::standard`.

## Part library and offline work

Each board keeps its own copy of every footprint and model in `cache/`. A clone of the
board needs no network. Each download also goes into the part library on your computer:

- `$LAMINA_LIB`, or `%LOCALAPPDATA%\lamina\lib` on Windows, or `~/.lamina/lib`.
- The `lib` folders of a workspace are searched first. Put shared parts there and commit them.

An import looks in the project cache, then in the library, then asks the provider.
`search_parts` uses the library when a provider does not answer, and says so. To work
without a network, fill the library first:

```
lamina fetch lcsc:C6186 lcsc:C14663 kicad:LED_SMD:LED_0603_1608Metric
lamina library
```

The `fetch_library` and `library` tools do the same from an agent.

The 3D views use the model files in `cache/models/`. When a file is not there, they use
the copy in the library. `status` lists each footprint whose model file is missing from
`cache/models/`, in its list of missing models. This happens when the folder was not committed or
was deleted. To get the files back:

```
lamina restore-models <board>
lamina restore-models <board> --no-download
```

The `restore_models` tool does the same from an agent. It copies each missing model from
the library. If the library has no copy, it downloads the model again from the provider
that the footprint came from. Use `--no-download` or `download: false` to use only the
library. Footprints, parts and placement do not change, and a model file that is in
`cache/models/` is never replaced. A model path that leaves `cache/models/` (`..` or an
absolute path) is refused. When a provider now gives a model with a different file name,
the tool does not change the footprint: import the part with `refresh: true` to use the
new model.

`import_part` and `import_parts` also restore the model of a part whose footprint is in
the project already. They keep the footprint as it is.

## KiCad files

Lamina reads and writes KiCad files. You do not need KiCad.

- The `kicad` provider imports footprints (see Providers). Lamina finds local libraries
  through `kicad_footprints` and `kicad_models` in `lamina.json`, `KICAD*_FOOTPRINT_DIR`
  and `KICAD*_3DMODEL_DIR`, or a KiCad install. If there is no local library, it downloads
  the footprint and its STEP model from the official KiCad libraries.
- `import_board` and `lamina import <file> <dir>` import a `.kicad_pcb` from KiCad 6
  to 10 (format `kicad`). Lamina imports the outline, footprints, parts, nets, placement,
  tracks, vias, zones, keep-outs and texts. The design rules and net classes come from
  the `.kicad_pro` file next to the board. Lamina finds `${KIPRJMOD}` models next to the
  board. The `kicad` provider gives the other models, from the local libraries or from
  the official online libraries. `--no-models` skips them.
- `export_board` with `format: kicad` and `lamina export-board <dir> --format kicad`
  write a `.kicad_pcb` for KiCad 8 or later. KiCad fills the zones again when it opens
  the file.

## Board formats

Lamina reads a board file through a board format. `formats` and `lamina formats` list
them: `kicad`, `easyeda` and `gerber`. `import_board` and `lamina import <file> <dir>`
detect the format from the file, or take it with `format`.

## EasyEDA boards

`import_board` and `lamina import <file.json> <dir>` import a board from the standard
EasyEDA editor (format `easyeda`). In EasyEDA, use File → Export → EasyEDA to save the file.

Lamina imports the outline, the parts with their LCSC numbers, nets, placement, tracks,
vias, copper areas, holes and texts. Lamina downloads the 3D models from EasyEDA. Solid
copper shapes, for example teardrops, stay as drawings. Lamina makes the design rules
less strict to agree with the board. After the import, apply a fab preset.

## Gerber files

`import_board` and `lamina import <folder or zip> <dir>` import Gerber and Excellon
files from any CAD program (format `gerber`). Lamina finds the layer of each file from its X2
attributes or from its name (KiCad, Altium, EasyEDA and Eagle names).

Gerber files have no parts and no nets, so the board has only the outline, the holes,
and the copper and silkscreen as drawings. Use it to look at a board, to render it or to
compare it with a different version.

## STEP export

`export_step` and `lamina export-step <dir>` write the board and its parts as one STEP file
(AP214). You can open it in Fusion, FreeCAD, SolidWorks and other CAD programs, for example
to design an enclosure.

- The board is a solid with its outline, cut-outs and holes. `vias` also adds the via holes.
- Each part is a component at its position. Lamina copies the STEP file of each part without
  changes, so the shapes, colours and names stay the same.
- A part that has only an OBJ model goes in as flat faces.

## Other autorouters

Lamina does not need a different router. To compare with a different router, export the
board as a Specctra DSN file and import the result:

- `routers` and `lamina routers` list the routers Lamina exchanges files with (`specctra`).
- `export_for_router` and `lamina export-routing <dir> --router specctra` write the DSN file.
- `import_router_session` and `lamina import-routing <dir> <file.ses> --router specctra`
  read the session file.

With [Freerouting](https://github.com/freerouting/freerouting):

```
lamina export-routing boards/servo --router specctra
java -jar freerouting.jar -de boards/servo/output/esp32-servo.dsn -do boards/servo/output/esp32-servo.ses
lamina import-routing boards/servo boards/servo/output/esp32-servo.ses --router specctra
```

The DSN file has the existing tracks as fixed tracks. If you set `reroute_all`, Lamina does
not put them in the file.
