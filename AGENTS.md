# Zuzu Designer

This repository contains `zuzu-designer`, a Rust and GTK4 form builder for
creating Zuzu GUI XML.

Use Oxford English in documentation: mostly standard British English, with
`-ize` word endings.

## Relationship To Other Projects

`zuzu-designer` is a GUI authoring tool, not a language runtime. It depends
on the sibling `zuzu-rust` crate through Cargo for preview rendering. The
preview evaluates `std/gui.gui_from_xml` through the Rust runtime so GUI XML
behaviour should stay aligned with `zuzu-rust` and `stdlib`.

Do not implement runtime or stdlib shortcuts in the designer. If preview
behaviour exposes a language/runtime bug, fix it in `zuzu-rust` or `stdlib`
as appropriate.

## Project Shape

- `src/main.rs` contains the GTK application, tree model, XML editing,
  import/export, and preview wiring.
- `Cargo.toml` depends on GTK4, `roxmltree`, and `zuzu-rust`.
- The document root is a `Window` element in the Zuzu GUI XML namespace.

## Build And Run

Install GTK4 development packages first. On Debian or Ubuntu:

```bash
sudo apt install build-essential pkg-config libgtk-4-dev libgdk-pixbuf-2.0-dev
```

Then run from this repository:

```bash
cargo run
```

Or run with an existing XML file:

```bash
cargo run -- path/to/file.xml
```

For non-GUI validation, use:

```bash
cargo check
cargo test
```

## Style And Maintenance

Use `cargo fmt` for Rust files you touch. Keep GTK UI changes consistent
with the existing simple tree-plus-preview design. Avoid adding a hard
dependency from `zuzu-rust` back to the designer.
