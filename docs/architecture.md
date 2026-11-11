# Lamina architecture

The design document is in [specs/](specs/). For the tools and the viewer, read
the [user guide](guide.md).

## Crates

Lamina is a hexagon. The domain in the middle knows only the board. The core runs the
operations and talks to the outside through ports, which are traits. Adapters implement
the ports. The programs plug the adapters in. The compiler keeps the rule: `lamina-core`
does not depend on an adapter crate.

Domain:

| Crate | Contents |
|---|---|
| `lamina-model` | Data types, JSON schema, load and save of a project, the `Provider` port |
| `lamina-geom` | Polygon operations, spatial index |
| `lamina-footprint` | IPC-7351 footprints, footprints from a datasheet |
| `lamina-drc` | Design rules, fab presets, DRC, connectivity |
| `lamina-route` | Track routing, pour fill, autorouter |
| `lamina-scene` | Shapes to draw, for the 2D and 3D renderers |
| `lamina-ports` | The ports: `BoardFormat`, `Fabricator`, `MeshLoader`, `StepWriter`, `Renderer`, `DesignSource`, `RouterExchange`, and the `Ports` bundle |
| `lamina-core` | Project operations, one function for each tool. Calls ports only |

Adapters:

| Crate | Ports it implements |
|---|---|
| `lamina-parts` | `Provider` (LCSC / JLCPCB parts), `BoardFormat` (EasyEDA boards) |
| `lamina-kicad` | `Provider` (KiCad libraries), `BoardFormat` (`.kicad_pcb` read and write) |
| `lamina-export` | `BoardFormat` (Gerbers in), `Fabricator` (Gerber X2, Excellon, BOM, CPL), `StepWriter` |
| `lamina-step` | `MeshLoader` (STEP and OBJ readers, mesh cache) |
| `lamina-render` | `Renderer` (PNG images in 2D and 3D) |
| `lamina-build` | `DesignSource` (Jsonnet board sources) |
| `lamina-specctra` | `RouterExchange` (Specctra DSN and SES) |

Programs:

| Crate | Contents |
|---|---|
| `lamina-wiring` | The composition root: the standard adapters plugged into `Ports` |
| `lamina-mcp` | The `lamina` program: MCP server and CLI |
| `lamina-viewer` | The `lamina-viewer` program: 2D and 3D viewer and editor |

To add a source of parts, a file format or a fabrication output, write one type that
implements the port and add one line in `lamina-wiring`.

`vendor/jrsonnet-stdlib` is a copy of the Jsonnet standard library without YAML and
wasm-bindgen. The pinned versions of these two do not agree with egui and wgpu.

## Developer tools

- `cargo run -p lamina-export --example outline -- <file.gbr> [tolerance]` shows the closed
  loops in a Gerber outline file.
- `cargo run --release -p lamina-step --example corpus -- <dir>` loads all STEP files in a
  folder. It compares the size of each one with the OBJ file that has the same name.
- `cargo run -p lamina-render --example meshview -- <model> <out.png>` renders one model.

## Screenshots without a window

`lamina-viewer shot` runs the viewer without a window. It opens a board, plays input
steps (keys, clicks, drags, scrolls, typed text) and saves PNG images. Build it with the
`headless` feature:

```
cargo build --release -p lamina-viewer --features headless
lamina-viewer shot <board> --3d --size 1280x760 drag:560,560,600,560 --out view.png
```

`lamina-viewer shot --help` lists the steps. On a computer without a GPU, a software
Vulkan driver works (Mesa lavapipe, `mesa-vulkan-drivers` on Debian and Ubuntu).

`scripts/viewer-smoke.py` is the smoke test of `shot` that CI runs on Linux. It makes a
small board through `lamina mcp` (no network), takes 2D and 3D images with steps of each
kind and checks the images, the time, the memory and that the board loaded once
(`shot --stats`): a board that loads again and again is a reload loop.

## README images

The screenshots and the GIF in the README come from the real programs.
`scripts/readme-media.py` builds the example board with
[`examples/c3-sensor/build.py`](../examples/c3-sensor/build.py), takes the screenshots
with `lamina-viewer shot` and makes the GIF with Pillow and ffmpeg. After a change to
the viewer, run it again:

```
cargo build --release -p lamina-mcp
cargo build --release -p lamina-viewer --features headless
python3 scripts/readme-media.py
```

`render-layers.png` comes from the `render` tool, as an agent sees the board. The
`board-*.png` images are `render` images of an earlier ESP32-S3 servo board.
