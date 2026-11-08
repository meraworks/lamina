## Changes in 0.3.0

PCB layout and validation tools for AI agents:

- Structured custom rules select nets, net classes and copper layers. DRC,
  pours and routing apply the resolved limits.
- Configure DRC severities and exclude individual violations with recorded
  reasons. New checks cover mask openings near zones, holes inside courtyards,
  thermal spokes, silkscreen size/overlap/edge clearance, net lengths and pairs.
- Measure total net track lengths and via counts with `net_lengths`; configure
  length and skew limits. `board_stats` reports dimensions, area, components,
  pads, vias, drill sizes and track measurements. Both tools are read-only.
- Define differential pairs in projects or modular sources, check their gap and
  coupling, and route both nets from a centre path with `route_diff_pair`.
- `tune_length` adds meanders to a selected segment and refuses disconnected
  contacts or DRC errors. `add_teardrops` and `remove_teardrops` expose the
  existing copper operations through MCP.
- KiCad exports include filled zones and a project footprint library. Existing
  library table entries are preserved; conflicting library names refuse export.
- Fabrication archives include an IPC-D-356 electrical test netlist.

Validation: 259 workspace tests passed, with one real-board overview test ignored.
Headless MCP tests cover routing, tuning, teardrops, statistics and fabrication.
On the reference board, all 192 IPC test-point records match KiCad's type, layer
access and coordinates. KiCad DRC on exported fills reports no unconnected items
or footprint-library warnings; other board warnings remain.

Measurement limits: lengths sum all track branches and exclude via depth;
length/skew and pair checks do not calculate delay or impedance. Pair checks
allow short corner/end tolerances. KiCad may produce different thermal warnings
when it refills zones. IPC validation is not certification by every fabricator.

Each archive has two programs, the README and the licence files:

- `lamina`: the MCP server and the command line tool.
- `lamina-viewer`: the 2D and 3D viewer and editor.

| Platform | Archive |
|---|---|
| Windows x64 | `x86_64-pc-windows-msvc.zip` |
| Windows Arm64 | `aarch64-pc-windows-msvc.zip` |
| Linux x64 | `x86_64-unknown-linux-gnu.tar.gz` |
| Linux Arm64 | `aarch64-unknown-linux-gnu.tar.gz` |
| macOS Apple silicon | `aarch64-apple-darwin.tar.gz` |
| macOS Intel | `x86_64-apple-darwin.tar.gz` |

Each archive has a `.sha256` file with its checksum.

## Install

1. Download the archive for your platform and extract it.
2. Put the folder in a permanent location.
3. Connect your MCP client to `lamina mcp`. The README shows the commands for Claude Code and Codex.

## Known limits

- The programs are not signed.
  - On Windows, SmartScreen can show a warning. Click "More info", then "Run anyway".
  - On macOS, remove the quarantine flag before the first start: `xattr -d com.apple.quarantine lamina lamina-viewer`.
- On Linux, the viewer needs a Vulkan or OpenGL driver and an X11 or Wayland session. The Linux builds need glibc 2.35 or later.
