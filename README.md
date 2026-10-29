# Oxidraft

**Open-source 2D drafting in Rust, in the spirit of AutoCAD LT.**

Oxidraft is a 2D-only CAD app that should feel familiar to anyone who drafts in
AutoCAD LT: a ribbon, a command line with aliases and AutoComplete, object
snaps, polar tracking, dynamic input, grips and window/crossing selection, and
DWG and DXF files. The goal is to cover the commands that make up most day-to-day 2D
work (LINE, PLINE, OFFSET, TRIM, FILLET, LAYER, DIM, HATCH…).

> Status: early prototype (v0.9). Expect breaking changes.

## What works today

| Area | Supported |
|---|---|
| Draw | `LINE` (`L`), `PLINE` (`PL`) with tangent Arc / Second pt / Close / Undo, Width / Halfwidth (tapered segments) and Length, `CIRCLE` (`C`), `ARC` (`A`) by three points or Center/Start/End/Angle, `RECTANG` (`REC`) by corners or Dimensions, `POLYGON` (`POL`) inscribed, circumscribed or by Edge, `ELLIPSE` (`EL`) by axis or Center, with Rotation, and elliptical Arcs, `SPLINE` (`SPL`) through fit points or by control vertices (Method), open or Closed, `XLINE` (`XL`) with Hor/Ver/Ang/Bisect/Offset and `RAY`, `DONUT` (`DO`), `REVCLOUD` around a rectangle or polygon or from an Object, Normal or Calligraphy, `POINT` (`PO`) drawn in the `PDMODE`/`PDSIZE` style, `DIVIDE` (`DIV`) and `MEASURE` (`ME`) placing points or blocks along an object |
| Annotation | `MTEXT` (`T`) in a box with Height/Rotation/Width and `\P` paragraphs, single-line `TEXT` (`DT`), `DIMLINEAR` (`DLI`, horizontal/vertical from the cursor, or Rotated) and `DIMALIGNED` (`DAL`) with text override, `DIMCONTINUE` (`DCO`) and `DIMBASELINE` (`DBA`), `DIMRADIUS` (`DRA`), `DIMDIAMETER` (`DDI`), `DIMANGULAR` (`DAN`: two lines, an arc, a circle or three points), `MLEADER` (`MLD`); text, dimensions and leaders move, rotate, scale, stretch and edit in the Properties palette |
| Hatch | `HATCH` (`H`): pick inside any enclosed area — even one formed by separate overlapping lines — and islands inside are left out; or select closed objects. Patterns SOLID, ANSI31, ANSI37, LINE, NET with scale and angle; hatches draw behind other objects |
| Blocks | `BLOCK` (`B`) turns objects into a block reference, `INSERT` (`I`) with Scale/Rotate and a live preview, `EXPLODE` (`X`) one level; `BEDIT` (`BE`, the Block panel's Edit, or double-click a block) opens a block in the Block Editor on its own, its base point at the origin, with Save Block (`BSAVE`) and Close Block Editor (`BCLOSE`, which asks before throwing changes away); the Blocks palette (`BLOCKSPALETTE`, the ribbon's Insert) shows this drawing's blocks as previews to click and place, and a library folder of DWG and DXF drawings whose blocks, or whole drawings, come in with their layers and the blocks they nest; references move, rotate, scale and mirror like any object; attribute values edit in the Enhanced Attribute Editor (`EATTEDIT`, or double-click the block); `ATTDISP` shows them as set, all, or none. `XCLIP` (`XC`) shows only part of a block reference or xref: inside (or with Invert clip, outside) a rectangle, polygon or polyline, with ON/OFF, Delete and generate Polyline; the boundary moves with the reference, its frame shows on screen but doesn't plot, and it's saved as AutoCAD's spatial filter in DXF and DWG. `CLIP` (the Insert tab's Clip) clips whatever is picked: a reference, an image or a PDF. `REFEDIT` (Insert tab, or Edit Block/Xref In-place on the right-click menu) edits a reference where it sits: the rest of the drawing fades and can't be selected, what you draw joins the working set, `REFSET` (Add/Remove) moves objects in or out, and `REFCLOSE` (the Edit Reference panel's Save or Discard Changes) redefines the block, updating every reference, or throws the edits away |
| External references | `XATTACH` (`XA`, Insert tab): attach a DWG or DXF drawing with a full, relative or no path, as an attachment or overlay, at a picked or typed point, scaled and rotated. `IMAGEATTACH` (`IAT`): attach a GeoTIFF, TIFF, PNG, JPEG or BMP image, placed by its georeferencing (GeoTIFF tags or a world file such as `.tfw`) or by hand; pick many GeoTIFFs at once to attach them all where they belong, each on its own layer, turned off and not read: turning a layer on loads its image. `PDFATTACH`: attach a page of a PDF (at scale 1, its size in millimeters). `IMAGECLIP` (`ICL`, `PDFCLIP`): clip an image or PDF page to a rectangle or polygon. Each attached drawing, image or page goes on a layer of its own name. The External References palette (`XR`, `ER`) lists them all with status (Loaded, Unloaded, Not Found, Unreferenced), size, type and path, and the drawings each attachment attaches in turn; it opens a drawing in its own window, reloads, unloads, detaches and binds (Bind or Insert) drawings, and loads, unloads and detaches images, several at a time, or loads the images in view. Images are read in the background. Attached drawings draw faded; their layers come along as `NAME\|layer`, and turning them off, freezing or recoloring them is saved. Files are found again by their saved path or next to the drawing; only paths are saved. When a file has moved, the palette's Browse... picks where it is now (other missing images in that folder are found with it), and Find in Folder... repoints several images at once. `REFEDIT` on an attached drawing saves the edits to its own file (under that file's layer names), after saying what in it Oxidraft can't keep, and leaves the old file as `.bak` |
| Layers & properties | Layer Properties Manager (`LA`): new, delete, rename, current, On/Freeze/Lock, color — all undoable, for several layers at once (Cmd- and Shift-click); the ribbon layer dropdown has LT's bulb/sun/lock toggles and shows the selected objects' layer (pick another to move them there); `LAYMCUR` (Make Current) makes an object's layer current; `LAYISO` / `LAYUNISO` (lock and fade, or turn off, every other layer), `LAYOFF`, `LAYFRZ`, `LAYLCK`, `LAYULK`, `LAYON`, `LAYTHW` — also on the ribbon; locked layers draw faded and can't be selected. Properties palette (`PR`, Cmd+1, or double-click an object) edits color, layer, linetype, lineweight and geometry; it docks at the left or right of the drawing area or floats (drag its title, or its Dock menu), and stays where it was put; `MATCHPROP` (`MA`) |
| Linetypes & lineweights | per layer (in the Layer Properties Manager) or per object, ByLayer / ByBlock through blocks; AutoCAD's standard linetypes (DASHED, HIDDEN, CENTER, PHANTOM, DOT, DASHDOT, BORDER, DIVIDE) plus any a drawing defines (complex linetypes draw their dashes, not their text or shapes); `LTSCALE` (`LTS`); LWT on the status bar shows lineweights |
| Modify | `MOVE` (`M`), `COPY` (`CO`), `ROTATE` (`RO`) and `SCALE` (`SC`) with Copy/Reference, `MIRROR` (`MI`), `STRETCH` (`S`), `OFFSET` (`O`) by distance or Through (sharp outside corners), `TRIM` (`TR`) and `EXTEND` (`EX`) in Quick mode with Shift to switch, `FILLET` (`F`) and `CHAMFER` (`CHA`) for two lines or all polyline corners, `ARRAY` (`AR`, also `ARRAYRECT`, `ARRAYPOLAR`): rows and columns or around a center, `BREAK` (`BR`) and `BREAKATPOINT`, `DRAWORDER` (`DR`: Front, Back, Above or Under objects; also on the Modify panel and the right-click menu), `TEXTTOFRONT`, `HATCHTOBACK` (new hatches go behind what they fill, attached images to the back; draw order is read from and saved to DXF and DWG), `JOIN` (`J`) into a line, arc or polyline, `LENGTHEN` (`LEN`: DElta, Percent, Total, DYnamic), `PEDIT` (`PE`): Close/Open, Join (Multiple with a fuzz distance), Width, Decurve, Reverse, and lines or arcs turned into polylines; TRIM, BREAK and OFFSET work on ellipses, splines, construction lines and rays (trimming a construction line leaves a ray), `EXPLODE` (`X`), `ERASE` (`E`), grip editing (click or drag a grip) |
| Selection | pick, window (left to right), crossing (right to left), click-click windows, press-drag lassos (dragged off to the right a window lasso, to the left a crossing one, which STRETCH stretches by too), selection cycling (`SELCYCLE` on the status bar, Ctrl+W: a click on objects on top of each other lists them, each highlighted as you point at it, to pick the one meant, at selection prompts and at prompts for one object such as OFFSET's, TRIM's or FILLET's), selection preview, Shift to remove, noun-verb (select first, then run a command), Delete erases; `SELECTSIMILAR` (same type and layer, or what its SEttings choose); Quick Select (`QSELECT`, also in the right-click menu and the Properties palette) by object type and color, layer, linetype or lineweight, to include, exclude or append; Cmd+C / Cmd+X / Shift+Cmd+C (`COPYCLIP`, `CUTCLIP`, `COPYBASE`) and Cmd+V (`PASTECLIP`, `PASTEORIG`) copy objects with their layers and blocks, between drawings too |
| Inquiry | `DIST` (`DI`), `AREA` (`AA`) of picked points or an object (circles, ellipses, splines, polylines, arcs, hatches without their islands), `ID`, `LIST` (`LI`) |
| Layouts | the tabs beside Model switch to each layout (paper space): its sheet with the printable area marked, the model shown through its viewports at their scale without the layers they freeze, and what's drawn there going onto the sheet; each space keeps its own view. A drawing without layouts gets Layout1, an A4 sheet, when its tab is first opened. A layout tab's right-click menu makes a New Layout, and Deletes (after asking), Renames (or double-click the tab), Copies, sets up the page of or plots a layout; `LAYOUT` (`LO`) does the same on the command line (Copy/Delete/New/Rename/Set/?). `PAGESETUP` picks a layout's paper and orientation; on the Model tab it sets the Plot dialog's settings for the model's plots without plotting. On a layout the ribbon brings up its Layout tab: New, Page Setup, and Rectangular viewports, Lock, Unlock and Maximize. MVIEW (MV) makes viewports from two corners or filling the printable area, and turns them on, off or locked. Double-clicking inside a viewport (MSPACE) works in model space through it with the sheet around it, zooming and panning its view unless it's locked; double-clicking the sheet (PSPACE) goes back. VPMAX fills the drawing area with a viewport, VPMIN puts it back. Layouts, their page setups, paper space objects and viewports are kept when DWG and DXF files are opened and saved |
| Plot | `PLOT` (Cmd+P, also on the Output tab and in the app menu): AutoCAD's Plot dialog — ISO A4–A0 and ANSI A–E paper, portrait or landscape, plot Display, Extents or a Window, Fit to paper or a scale like 1:500, centered or offset, monochrome.ctb or grayscale.ctb, object lineweights — with a sheet preview; `PREVIEW` shows the page full-screen. On a layout's tab it plots the Layout: its sheet at 1:1 on the paper of its page setup, with model space through each viewport at its scale, without the layers it freezes, cut to its frame, and linetypes as long as on the sheet (PSLTSCALE 1). Writes a vector PDF whose text is set in the drawing area's typeface, so it can be searched and copied |
| History | `UNDO` (`U`), `REDO`; each command undoes as one step |
| Command line | floating, AutoComplete (Up/Down/Tab), clickable options, Up-arrow recall, Enter/Space to confirm, Enter repeats the last command, F2 expands a scrolling history |
| Shortcut menus | right-click in the drawing: Repeat / Recent Input, Undo/Redo, Pan, Zoom, Select All (Cmd+A), Quick Select when idle; Clipboard (Cut, Copy, Copy with Base Point, Paste, Paste to Original Coordinates); Erase, Move, Copy, Rotate, Scale, Mirror, Draw Order, Edit Xref (or Block) In-place, Clip Xref, Select Similar, Deselect All with objects selected; Enter, Cancel, the prompt's options, one-shot Snap Overrides, Pan and Zoom while a command runs |
| Input | `x,y`, `@dx,dy`, `@d<angle`; with dynamic input on, next points are relative unless prefixed `#`; direct distance entry |
| Drafting aids | object snaps (F3): endpoint, midpoint, center, node, quadrant, intersection, insertion, perpendicular, tangent, nearest (on ellipses and splines too) — choose the running ones from the arrow beside OSNAP, or a one-shot override from the right-click menu; polar tracking (F10); ortho (F8); dynamic input (F12); grid (F7); snap mode (F9, or `SNAP` to set its spacing): points land on the snap spacing where no object snap applies |
| View | mouse wheel zooms at the cursor, middle-drag pans, middle double-click zooms extents; trackpad: two-finger pan, pinch zoom; `ZOOM` (`Z`): pick two corners for a window, a scale (`2x`, or a plain number times the extents), All, Center, Extents (also `ZE`), Previous (wheel zooms and pans count too), Scale, Window, Object, Enter for real time; `PAN` (`P`) drags with the left button until Esc or Enter; a navigation bar with Pan and Zoom at the right of the drawing |
| Files | DWG open/save, AutoCAD R14 to 2018 files (saved back in the version they were opened in; new drawings save as AutoCAD 2018); damaged DWGs open as far as they can be read. DXF open/save: lines, circles, arcs, ellipses, splines, construction lines and rays, polylines with arc segments and widths (LWPOLYLINE, and old-style POLYLINE on import), blocks and block references with attributes, external references, images (IMAGE) and PDF underlays (PDFUNDERLAY) with their clipping, MTEXT/TEXT, linear, radius, diameter and angular dimensions, leaders, points with PDMODE/PDSIZE, hatches (HATCH; patterns Oxidraft doesn't have show as ANSI31), 2D solids (SOLID, as solid fills); layers with their color, linetype, lineweight and on/frozen/locked states; linetype definitions and LTSCALE. Saves as DXF R2000. A leader's text comes back as separate text |
| Options | `OPTIONS` (`OP`, also in the app menu): AutoCAD's dark or light color theme for the ribbon, palettes and command line (`COLORTHEME` 0 or 1), and a dark or white drawing area, where color 7 draws black as in AutoCAD; kept between runs |
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
| `oxidraft-io-dwg` | DWG import/export (via [`acadrust`](https://crates.io/crates/acadrust), through DXF in memory) |
| `oxidraft-plot` | page setup, the plotted sheet, and PDF output |
| `oxidraft-raster` | image and PDF underlays: GeoTIFF and world-file placement, decoding (via [`image`](https://crates.io/crates/image), [`tiff`](https://crates.io/crates/tiff) and [`hayro`](https://crates.io/crates/hayro)) |
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

- **v0.10:** layouts and viewports, plotting from them

## Contributing

Contributions are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md). Each
command is a self-contained task, which makes it a good first issue.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

DWG files are read and written with [acadrust](https://github.com/hakanaktt/acadrust),
used unmodified under the [Mozilla Public License 2.0](https://www.mozilla.org/MPL/2.0/);
its source is available from its repository and from crates.io. Plotted PDFs embed the
Ubuntu Light typeface, under the [Ubuntu Font Licence](https://ubuntu.com/legal/font-licence).

Oxidraft is an independent project and is not affiliated with or endorsed by
Autodesk. AutoCAD is a trademark of Autodesk, Inc.
