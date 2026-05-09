# zuzu-designer

`zuzu-designer` is a Rust and GTK4 form builder for creating Zuzu GUI XML.

The designer keeps a `Window` element as the root of the document. Use the
tree pane to add, reorder, remove, and edit child elements. Right-click a tree
node, or double-click it, to edit its XML properties. The preview pane renders
the generated XML by embedding `zuzu-rust` and evaluating `std/gui.gui_from_xml`,
then wrapping the runtime's GTK widget preview in the designer UI.

## Build

Install GTK4 development packages first. On Debian or Ubuntu:

```sh
sudo apt install build-essential pkg-config libgtk-4-dev libgdk-pixbuf-2.0-dev
```

Then run:

```sh
cargo run --manifest-path extras/zuzu-designer/Cargo.toml
```

The crate is intentionally separate from `extras/zuzu-rust` so the existing
runtime build does not gain a hard GTK4 compile-time dependency.
