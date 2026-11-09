# Contributing to Lamina

Thank you for your interest. Bug reports, board files that Lamina reads badly, ideas
for tools and pull requests are all welcome.

## Report a problem

Open an issue with:

- what you did (the tool calls of the agent, or the steps in the viewer),
- what you expected and what happened,
- the board folder, or a small board that shows the problem, if you can share it,
- your system and the Lamina version (`lamina --version`).

## Build and test

You need [Rust](https://rustup.rs) 1.90 or later.

```sh
cargo build --release
cargo test --workspace
```

Before a pull request, run the same checks as CI:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

[docs/architecture.md](docs/architecture.md) explains the crates. In short: the domain
crates know only the board, `lamina-core` talks to the outside through ports, and an
adapter crate implements each port. A new part source, file format or fab output is one
type that implements a port, plus one line in `lamina-wiring`.

## Try a change in the real programs

`examples/c3-sensor/build.py` builds a complete board through the MCP tools in about
ten seconds. It is a quick way to see an end-to-end run:

```sh
python3 examples/c3-sensor/build.py --lamina target/release/lamina --out /tmp/c3
target/release/lamina-viewer /tmp/c3
```

## Update the README images

The README images come from the real programs. If you change how the viewer looks,
make them again:

```sh
cargo build --release -p lamina-mcp
cargo build --release -p lamina-viewer --features headless
python3 scripts/readme-media.py
```

## Licence

Lamina is MIT OR Apache-2.0. Unless you say otherwise, a contribution that you submit
is licensed in the same way, without other terms.
