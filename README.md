# Oxidraft

**Open-source 2D drafting in Rust, in the spirit of AutoCAD LT.**

Oxidraft is a 2D-only CAD app that should feel familiar to anyone who drafts in
AutoCAD LT: a ribbon, a command line with aliases and AutoComplete, object
snaps, polar tracking, dynamic input, grips and window/crossing selection, and
DXF files. The goal is to cover the commands that make up most day-to-day 2D
work (LINE, PLINE, OFFSET, TRIM, FILLET, LAYER, DIM, HATCH…).

> Status: early prototype (v0.5). Expect breaking changes.

## What works today

| Area | Supported |
|---|---|
| Draw | `LINE` (`L`), `PLINE` (`PL`) with tangent Arc / Second pt / Close / Undo, `CIRCLE` (`C`), `ARC` (`A`) by three points or Center/Start/End/Angle, `RECTANG` (`REC`) by corners or Dimensions |
| Annotation | `MTEXT` (`T`) in a box with Height/Rotation/Width and `\P` paragraphs, single-line `TEXT` (`DT`), `DIMLINEAR` (`DLI`, horizontal/vertical from the cursor, or Rotated) and `DIMALIGNED` (`DAL`) with text override, `DIMCONTINUE` (`DCO`) and `DIMBASELINE` (`DBA`), `DIMRADIUS` (`DRA`), `DIMDIAMETER` (`DDI`), `DIMANGULAR` (`DAN`: two lines, an arc, a circle or three points), `MLEADER` (`MLD`); text, dimensions and leaders move, rotate, scale, stretch and edit in the Properties palette |
| Hatch | `HATCH` (`H`): pick inside any enclosed area — even one formed by separate overlapping lines — and islands inside are left out; or select closed objects. Patterns SOLID, ANSI31, ANSI37, LINE, NET with scale and angle; hatches draw behind other objects |
| Blocks | `BLOCK` (`B`) turns objects into a block reference, `INSERT` (`I`) with Scale/Rotate and a live preview, `EXPLODE` (`X`) one level; references move, rotate, scale and mirror like any object |
| Layers & properties | Layer Properties Manager (`LA`): new, delete, rename, current, On/Freeze/Lock, color — all undoable; the ribbon layer dropdown has LT's bulb/sun/lock toggles; locked layers draw faded and can't be selected. Properties palette (`PR`, Cmd+1, or double-click an object) edits color, layer, linetype, lineweight and geometry; `MATCHPROP` (`MA`) |
| Linetypes & lineweights | per layer (in the Layer Properties Manager) or per object, ByLayer / ByBlock through blocks; AutoCAD's standard linetypes (DASHED, HIDDEN, CENTER, PHANTOM, DOT, DASHDOT, BORDER, DIVIDE) plus any a drawing defines (complex linetypes draw their dashes, not their text or shapes); `LTSCALE` (`LTS`); LWT on the status bar shows lineweights |
| Modify | `MOVE` (`M`), `COPY` (`CO`), `ROTATE` (`RO`) and `SCALE` (`SC`) with Copy/Reference, `MIRROR` (`MI`), `STRETCH` (`S`), `OFFSET` (`O`) by distance or Through (sharp outside corners), `TRIM` (`TR`) and `EXTEND` (`EX`) in Quick mode with Shift to switch, `FILLET` (`F`) and `CHAMFER` (`CHA`) for two lines or all polyline corners, `ARRAY` (`AR`, also `ARRAYRECT`, `ARRAYPOLAR`): rows and columns or around a center, `BREAK` (`BR`) and `BREAKATPOINT`, `JOIN` (`J`) into a line, arc or polyline, `LENGTHEN` (`LEN`: DElta, Percent, Total, DYnamic), `EXPLODE` (`X`), `ERASE` (`E`), grip editing (click or drag a grip) |
| Selection | pick, window (left to right), crossing (right to left), press-drag windows, selection preview, Shift to remove, noun-verb (select first, then run a command), Delete erases; `SELECTSIMILAR` (same type and layer, or what its SEttings choose); Quick Select (`QSELECT`, also in the right-click menu and the Properties palette) by object type and color, layer, linetype or lineweight, to include, exclude or append |
| Inquiry | `DIST` (`DI`), `AREA` (`AA`) of picked points or an object (circles, polylines, arcs, hatches without their islands), `ID` |
| History | `UNDO` (`U`), `REDO`; each command undoes as one step |
| Command line | floating, AutoComplete (Up/Down/Tab), clickable options, Up-arrow recall, Enter/Space to confirm, Enter repeats the last command, F2 expands history |
| Shortcut menus | right-click in the drawing: Repeat / Recent Input, Undo/Redo, Pan, Zoom, Select All (Cmd+A), Quick Select when idle; Erase, Move, Copy, Rotate, Scale, Mirror, Select Similar, Deselect All with objects selected; Enter, Cancel, the prompt's options, one-shot Snap Overrides, Pan and Zoom while a command runs |
| Input | `x,y`, `@dx,dy`, `@d<angle`; with dynamic input on, next points are relative unless prefixed `#`; direct distance entry |
| Drafting aids | object snaps (F3): endpoint, midpoint, center, quadrant, intersection, insertion, perpendicular, tangent, nearest — choose the running ones from the arrow beside OSNAP, or a one-shot override from the right-click menu; polar tracking (F10); ortho (F8); dynamic input (F12); grid (F7) |
| View | mouse wheel zooms at the cursor, middle-drag pans, middle double-click zooms extents; trackpad: two-finger pan, pinch zoom; `ZOOM` (`Z`): pick two corners for a window, a scale (`2x`, or a plain number times the extents), All, Center, Extents (also `ZE`), Previous (wheel zooms and pans count too), Scale, Window, Object, Enter for real time; `PAN` (`P`) drags with the left button until Esc or Enter; a navigation bar with Pan and Zoom at the right of the drawing |
| Files | DXF open/save: lines, circles, arcs, polylines with arc segments (LWPOLYLINE, and old-style POLYLINE on import), blocks and block references with attributes, MTEXT/TEXT, linear, radius, diameter and angular dimensions, leaders, hatches (HATCH; patterns Oxidraft doesn't have show as ANSI31), 2D solids (SOLID, as solid fills); layers with their color, linetype, lineweight and on/frozen/locked states; linetype definitions and LTSCALE. Saves as DXF R2000. A leader's text comes back as separate text |
| Updates | checks GitHub Releases at startup (set `OXIDRAFT_NO_UPDATE_CHECK=1` to skip); one click downloads, verifies the SHA-256 checksum and replaces the app, then restarts it |

Greyed-out ribbon buttons mark commands that are planned but not built yet.

## Install

From the [Releases page](https://github.com/protoblade/Oxidraft/releases):

- **Windows:** run `Oxidraft-<version>-windows-x64-setup.exe`. It installs for
  your user only (no admin rights), adds a Start menu entry and an uninstaller,
  and can make Oxidraft open `.dxf` files. Prefer no installer? Unzip
  `oxidraft-<version>-windows-x64.zip` and run `oxidraft.exe`.
- **macOS / Linux:** extract `oxidraft-<version>-<target>.tar.gz` and run
  `oxidraft`.

The builds aren't code-signed yet. Windows SmartScreen may warn on first run
(choose *More info* → *Run anyway*); on macOS a browser download may need
`xattr -d com.apple.quarantine oxidraft`. After that the app updates itself.

## Run from source

```bash
cargo run --release -p oxidraft-app
```

Open a file directly:

```bash
cargo run --release -p oxidraft-app -- path/to/drawing.dxf
```

Make a sample drawing to play with:

```bash
cargo run -p oxidraft-io-dxf --example demo -- demo.dxf
```

Render the UI headlessly in a few typical states (useful when reviewing UI changes):

```bash
cargo run -p oxidraft-app --example screenshots -- screenshots/
```

## Releasing

1. Bump `version` under `[workspace.package]` in `Cargo.toml`.
2. Commit, then tag and push: `git tag v0.2.0 && git push origin main v0.2.0`.

The [Release workflow](.github/workflows/release.yml) builds macOS (Apple
Silicon and Intel), Linux and Windows, and publishes
`oxidraft-<version>-<target>.tar.gz` plus a `.sha256` for each — the names the
in-app updater looks for. The updater only sees releases of a public repository.
For Windows it also builds the installer ([Inno Setup script](packaging/windows/oxidraft.iss))
and a zip, and installs, runs and uninstalls the app on the runner before
anything is published. The app icon comes from `python3 packaging/make_icon.py`.

## Workspace layout

| Crate | Purpose |
|---|---|
| `oxidraft-geom` | 2D points, line and arc segments, intersections, offsets (polylines via [`cavalier_contours`](https://crates.io/crates/cavalier_contours)) |
| `oxidraft-model` | the document: entities, layers, transactions/undo, grips, snaps and selection |
| `oxidraft-commands` | the command engine and every drafting command |
| `oxidraft-io-dxf` | DXF import/export (via the [`dxf`](https://crates.io/crates/dxf) crate) |
| `oxidraft-app` | desktop app built on [`egui`/`eframe`](https://github.com/emilk/egui): ribbon, canvas, command line, status bar |

### How a command works

Each command is a small state machine implementing `Command`
(`crates/oxidraft-commands/src/lib.rs`): it returns a `Prompt`, receives an
`Input` (point, value, keyword, selection or Enter), edits the `Document`, and
reports whether it needs more input. The `Engine` parses typed text, owns the
selection set, wraps each run in one undo transaction, and applies ortho/polar
tracking. See `line.rs` for a complete example, and `move_copy.rs` for one
that starts with "Select objects:".
Adding a command is usually one new file plus one entry in `COMMANDS`.

## Roadmap

- **v0.7:** layouts, viewports, plot to PDF/SVG

## Contributing

Contributions are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md). Each
command is a self-contained task, which makes it a good first issue.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Oxidraft is an independent project and is not affiliated with or endorsed by
Autodesk. AutoCAD is a trademark of Autodesk, Inc.
