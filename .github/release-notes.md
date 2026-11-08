## Changes in 0.2.2

Design rule check:
- New warning `silk_to_pad`: silkscreen text or a silkscreen drawing is nearer to a pad with a mask opening than the `silk_to_pad` rule. It checks board items and footprint items on both sides. A footprint outline needs to clear only the mask openings of its own pads.
- The Checks list in the viewer shows the warning. Click a row to zoom to the pad.

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
