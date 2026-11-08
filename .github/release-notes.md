## Changes in 0.2.1

Viewer, 2D:
- Pin names and nets on the pads, fitted to each pad. Net names on tracks.
- Net highlight (`` ` ``), single-layer mode (Shift+S), zoom keys (+ / -).
- Rotate while you drag (R); a group turns about the part that you hold.
- Box selection by track segment; a crossing box takes what it touches.
- Copy and paste of tracks and vias (Ctrl+C / Ctrl+V).
- Routing modes (Shift+R): direct, walkaround and push, with a live preview. A track or part drag pushes other tracks aside in push mode.
- Differential pairs (Shift+D) with vias and matched lengths; length tuning with meanders ("tune to" in the inspector); teardrops (Board menu).
- The active copper layer is the bright one. Track width keys while routing (W / Shift+W).

Viewer, 3D:
- Zoom at the pointer; top, bottom and reset views; part bodies on or off (T / B / I / P).
- Hover and click parts; 2D and 3D share one selection.

Tools:
- `pins`: the pads of a part with their pin names and nets. A rule check warns when a ground pin is on a power net, or the reverse.
- `render`: `overview: true` adds a board overview with the close-up marked. In the 2D layers view, `focus` zooms to a pad or part.
- `lamina-viewer shot` (build with `--features headless`): the viewer without a window, for agents and tests.

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
