# Features

What ResinSupport does today, as user stories by category. Read it before writing a prompt: it says what already exists, what to build on, and what is still missing. `ROADMAP.md` tracks the work; `docs/architecture.md` explains how each feature is built.

Each story reads:

> **As a** maker, **I want** … **so that** …
> - what the user sees and can do, including the edge cases
> - Command: `id` · Keys: …

Each category ends with **Not yet**: stories nobody has built, taken from `ROADMAP.md`.

## Viewing

### Navigate the model
**As a** maker, **I want** to orbit, pan and zoom around the plate with the mouse I'm used to, **so that** I can inspect a model from any side without thinking about the controls.
- Mouse mappings ported from SolidCraft: Fusion (default), Blender, SolidWorks and more, chosen in Preferences.
- The wheel zooms toward the point under the cursor; Ctrl+left drag orbits and Space+left drag pans in every mapping; a middle double-click goes home.
- Trackpad: two fingers pan, Shift+two fingers orbit, pinch zooms.

### Standard views
**As a** maker, **I want** to jump to a front, top or iso view and fit everything on screen, **so that** I can get back to a known framing after wandering off.
- Front, back, left, right, top, bottom and iso, animated; F fits all; P toggles perspective and orthographic.
- Arrows turn the view 15° (Shift: 90°), Ctrl+arrows pan.
- Commands: `view.standard`, `view.fit`, `view.projection`, `view.turn`

### Know where the plate is
**As a** maker, **I want** the printer's build plate drawn under my models with X/Y/Z axes, **so that** I can see what fits and tell the model's orientation and size at a glance.
- The plate is an opaque surface the size of the printer's display, with a grid every 10 mm from its centre; an axis gizmo shows the orientation.
- An arrow outside the plate's front edge (−Y) points into it, so the printing direction of the models reads at a glance.
- Seen from below, the plate turns see-through, so the models and their supports stay in view while editing supports.
- The `build_plate` preference (General) off brings back the ground grid whose spacing follows the zoom, with the plate's outline.

**Not yet**
- **As a** maker, **I want** a navigation cube I can click, **so that** I can change view without the keyboard.
- **As a** maker with a 3D mouse, **I want** it to drive the camera.

## Files

### Import a model
**As a** maker, **I want** to bring in an STL file however it reaches me, **so that** I can start preparing it right away.
- Binary and ASCII STL; from the File menu (Ctrl+I), by drag and drop, or on the command line.
- A broken or hostile file is refused with a message, never a crash.
- The view fits the new model; it is lifted and arranged on the plate (unless the `arrange_on_import` preference is off).
- Command: `file.import_stl`

### Save and reopen a project
**As a** maker, **I want** to save my models with their supports and reopen them later, **so that** I can split preparation over several sessions.
- `.rsup` project: models, their names, placements, supports (with any hand-edited corners), braces, labels, per-model slicing settings and which models are hidden; each mesh stored once.
- Ctrl+S saves, Ctrl+Shift+S saves as, Ctrl+O opens. The old file is kept as `.bak`, and a write never leaves a half-written file.
- A project opens exactly where it was saved, under my own support and print settings. A project from a newer version is refused with a message.
- Commands: `file.save`, `file.open`

### Copy models
**As a** maker, **I want** to copy a supported model and paste it, **so that** I can print several of the same part without supporting each one.
- Ctrl+C / Ctrl+V in Object edition; supports, braces, label and slicing settings come along.
- Commands: `edit.copy`, `edit.paste`

### Start over
**As a** maker, **I want** to clear the scene in one go (File › Clear Scene), **so that** I can start a new plate. It can be undone.
- Command: `scene.clear`

**Not yet**
- **As a** maker, **I want** to import OBJ or 3MF files.
- **As a** maker, **I want** to import a CTB file, **so that** I can inspect someone else's sliced print.
- **As a** maker in a browser, **I want** to import from a file picker, **so that** the web build works (`file.import_stl` takes a path today).

## Printer

### Describe my printer
**As a** maker, **I want** to enter my printer's build volume, resolution, pixel size and mirroring, **so that** the slices match its screen.
- Lives in the print settings card's Printer tab; kept in the preferences, never an undo step.
- Command: `print.configure`

### Choose the file format the printer reads
**As a** maker, **I want** to pick CTB v2 or encrypted CTB v5, **so that** my printer's firmware accepts the file.
- Printer tab › CTB format; encrypted v5 is the default.

**Not yet**
- **As a** maker, **I want** portrait screens, gamma or exposure remapping, and XY/Z scale correction, **so that** dimensions come out right on my machine.

## Placing models

### Select models
**As a** maker, **I want** to select a model by clicking it, its supports, or its row on the Models card, **so that** I can act on it.
- Shift toggles; Ctrl+A selects all; a model and its supports are one object.
- Box selection: dragged right takes what lies wholly inside, dragged left what it touches; Shift adds to the selection.
- Command: `model.select`

### Move, rotate, scale
**As a** maker, **I want** a gizmo and a live panel for moving, rotating and scaling, **so that** I can orient a part for printing.
- T, R, S open the tools; the gizmo is ported from SolidCraft; values are measured from the model as imported.
- Supports follow their model.
- A whole drag is one undo step.
- Commands: `model.move`, `model.rotate`, `model.scale`, `model.place`

### Type an exact transform
**As a** maker, **I want** to type `R 90` or `T X 50` with a tool open, **so that** I can get exact values without aiming with the mouse.
- Shown live; Enter applies and closes; Esc puts it back.

### Keep models lifted off the plate
**As a** maker, **I want** models held at a set height above the plate (5 mm by default), **so that** there is room for supports underneath.
- Kept through import, rotation and scaling, until I move the model up or down; set on the object settings card.
- Commands: `model.lift`, `scene.lift_height`

### Arrange the plate
**As a** maker, **I want** models spread out on the plate automatically, **so that** they don't overlap.
- Each goes to the free spot nearest the plate's centre, moved and turned by quarter turns about Z, never scaled.
- Command: `model.arrange`

### Delete a model
**As a** maker, **I want** Delete to remove the selected models with their supports.
- Del, the ribbon's Delete, or Delete in a model's menu on the Models card.
- Command: `model.delete`

### The Models card
**As a** maker, **I want** every model listed on a card I can fold or move, **so that** I can find, name and put aside the parts on my plate.
- A row per model; its eye hides the model, its supports and its raft, which are then neither drawn, picked, selected, sliced nor exported, and kept hidden in the project. Hiding and showing are instant: nothing is rebuilt.
- A double click or F2 renames a model in its row: Enter keeps the name, Esc puts it back.
- A right click opens its menu: Duplicate (Ctrl+D) adds a copy with its supports beside the others, Rename, Delete; on a selected model it acts on the whole selection.
- Commands: `model.visibility`, `model.rename`, `model.duplicate`

**Not yet**
- **As a** maker, **I want** to lay a chosen face flat on the plate, **so that** I can orient a model in one click.
- **As a** maker, **I want** an Arrange button and mirroring.
- **As a** maker, **I want** to be warned about open edges, inverted normals and self-intersections, **so that** I don't slice a broken mesh.

## Hollowing

**Not yet**
- **As a** maker, **I want** to hollow a model and add drain holes, **so that** I can save resin and avoid suction cups.

## Supports

### Place tips by hand
**As a** maker, **I want** to click the model to place a support tip, with a preview under the pointer, **so that** I can support exactly the points I care about.
- In Support edition (Tab); a 0.5 mm tip; a click on a tip selects it (Shift toggles); Delete removes it.
- Box selection works on supports too.
- Commands: `support.add`, `support.select`, `support.delete`

### Choose how strong supports are
**As a** maker, **I want** Light, Medium and Heavy presets for tip, body, base and raft sizes, **so that** I can match supports to the part's weight.
- Edited on the support settings card, checked against each other (a tip never wider than its body), kept in the preferences; never an undo step.
- Commands: `support.configure`, `support.reset_preset`

### Get a support down to the plate
**As a** maker, **I want** every tip to get a body down to the plate that keeps clear of my models, **so that** I only have to choose where to touch the model.
- Straight down, or with one knee no steeper than the preset's max tilt, keeping `body.clearance` from every model.
- A tip with no valid path is kept and shown in a warning colour, never refused: I may still move the model.

### Edit a support's path
**As a** maker, **I want** to select a support's joints and segments and drag them, **so that** I can route around a feature by hand.
- Handles at the head and each knee, drawn over everything; move arrows on the selection.
- An edited support keeps its corners (only its tip follows the model); it is checked and flagged but never routed again.
- Commands: `support.place_joints`, `support.reset_route`

### Branch from a support
**As a** maker, **I want** several tips to share one body, **so that** a cluster of points uses less resin and leaves fewer marks.
- Double-press a tip, then each press on a surface adds a tip to that support's head, until Esc.
- Any tip can be deleted; the body goes with the last one.

### Move a tip
**As a** maker, **I want** to move a placed tip to a better spot, **so that** I don't have to delete it and route it again.
- Press the contact handle of a selected support, then press the new spot; any other press or Esc puts it back.
- Command: `support.move_tip`

### Lay a line of tips
**As a** maker, **I want** to lay evenly spaced tips between two points, **so that** a long edge gets supported without clicking each tip.
- L with two tips selected (or one, or none for the newest, then a press on a surface for the other end); lines chain on.
- Shift+wheel or the card sets the spacing; tips are cast onto the surface; Enter adds them as one undo step.
- Command: `support.add_line`

### Brace supports together
**As a** maker, **I want** struts between neighbouring supports, **so that** tall supports don't sway or snap during peeling.
- H with supports selected; pairs them by chain, spanning tree, or every pair within a reach.
- Zig-zag or crossed struts at a set angle, section by section between knees, a share of the body's diameter, clear of heads and bases; struts that would come near a model are left out.
- Editing a braced support removes its braces.
- Command: `support.add_braces`

### Stand everything on a raft
**As a** maker, **I want** a raft under each model's bases, **so that** the print sticks to the plate and comes off in one piece.
- The bases' outline widened by a margin, with gaps up to a merge distance filled; straight, undercut or chamfered edge; optional rim. Drawn and printed.
- Each model has its own raft, never joined with another model's; where two overlap they print as one.

### Label prints on the raft
**As a** maker, **I want** a model's name or number standing out of its raft, **so that** I can tell calibration prints apart once they're off the plate.
- L in Object edition; several lines; on the front, back, left or right, read from that side; the raft widens to hold it.
- On the card, a Preset section (the preset, size, depth, margin) then an Options section (side, text); Delete removes it; saved in projects and copies.
- Commands: `model.label`, `model.label.delete`

### Reuse label settings as presets
**As a** maker, **I want** named presets of a label's size, depth and margin, **so that** every print I label looks the same without setting it up again.
- On the label card: pick a preset (or None) in the drop-down; the copy icon beside it saves what is shown as a new preset (a copy of the one used, named in place); a double click on the drop-down renames the preset in place; the delete icon deletes it once confirmed; kept with my preferences across projects.
- Changing a preset's settings on a model using it changes the preset and every model in the scene using it; the text and side stay each model's own.
- Saving, renaming and deleting a preset are not undone by Undo, nor is what they did to my labels (a renamed preset stays renamed on every model using it); projects not open keep the old name, shown as not in my presets.
- A deleted preset's labels keep their settings and its name, shown as not in my presets; the copy icon offers that name, taking them back.
- Commands: `label.preset.save`, `label.preset.list`, `label.preset.rename`, `label.preset.delete`, `model.label` (`preset`)

**Not yet**
- **As a** maker, **I want** overhangs and islands detected and supported automatically, **so that** I only touch up what the generator missed.
- **As a** maker, **I want** ribbon buttons for tip lines, braces and resetting a support's path, **so that** I don't need to know the shortcuts.
- **As a** maker, **I want** supports that stand on the model, or route with more than one knee, **so that** hard-to-reach tips still get support.

## Slicing

### Set print settings
**As a** maker, **I want** layer height, exposures and bottom layers on one card, **so that** I can tune a print to my resin.
- Kept in the preferences, never an undo step; Restore puts back the defaults.
- Commands: `print.configure`, `print.reset`, `print.settings`

### Tune lift and retract
**As a** maker, **I want** a slow then fast lift and a fast then slow retract, set separately for bottom and regular layers, **so that** peeling is gentle without slowing the whole print.
- UVtools' two-phase model; each phase side by side on the card; print time shown per phase.

### Slice the plate
**As a** maker, **I want** to slice in the background with progress and Cancel, **so that** the window stays usable on big plates.
- Slice edition (Tab); anti-aliased rasterizer ported from mslicer; supports, braces, raft and labels printed as drawn.
- Command: `print.slice`

### Smooth layer lines
**As a** maker, **I want** edge dithering on nearly vertical walls, **so that** the layer steps are less visible.
- Greys below what the resin cures are lit at that floor or left dark, spread across layers and along edges; each part has its own switch.
- Only where the surface leans less than a limit from vertical (20° by default).

### Calibrate with per-model settings
**As a** maker tuning a new resin, **I want** each model on the plate to have its own exposure and other slicing settings, **so that** I can compare several settings in one print.
- Any Slicing tab setting can be overridden for one model and its supports (Object Overrides, highlighted where they differ).
- Each model is sliced on its own and the layers combined by the brightest grey; saved in projects and copies.
- Resin estimates on a tab of their own.
- Command: `model.slicing`

### Check the layers
**As a** maker, **I want** to step through the layers in 3D and 2D, **so that** I can catch a problem before I spend resin.
- The layer slider; the 3D view cut at the layer and capped with its image; a 2D layer view; the layer's card shows exposure, lit area and lit regions.
- Page Up / Page Down (Shift: ten), Up / Down in the 2D view, Home / End.
- Models can be selected in the 3D view and in the 2D layer view.

**Not yet**
- **As a** maker, **I want** layers checked for islands, overhangs, resin traps and empty layers, **so that** the app warns me before I print.
- **As a** maker, **I want** waits before cure, after cure and after lift, **so that** I can follow my resin maker's recommendations.
- **As a** maker, **I want** elephant-foot compensation and a minimum feature width, **so that** bottom layers and thin details come out the right size.

## Export

### Write a file for the printer
**As a** maker, **I want** to export the sliced plate as a CTB file with previews, **so that** I can put it on a USB stick and print.
- Ctrl+E, the File menu, or Slice edition's ribbon; CTB v2 or encrypted v5; previews drawn by the renderer.
- Checked against UVtools 7.1.2, which reads the files back with the same settings and layers.
- Command: `file.export_ctb`

**Not yet**
- **As a** Saturn 2 owner, **I want** confirmation that the printer lists, shows and prints the exported files.
- **As a** maker with another printer, **I want** CTB v3/v4, `.goo`, or layer images in a zip.
- **As a** maker, **I want** per-layer settings in the exported file.

## Working

### Undo anything
**As a** maker, **I want** to undo and redo every change to the scene, **so that** I can experiment without fear.
- Ctrl+Z, Ctrl+Y (or Ctrl+Shift+Z); a drag or a typed field is one step; a tool's Cancel leaves the history alone; the last 200 steps.
- Settings changes are never steps, so undoing an edit never changes a setting.
- Commands: `edit.undo`, `edit.redo`

### Switch between object, support and slice work
**As a** maker, **I want** a mode for each stage of preparation, **so that** a click does what the stage needs.
- Tab or the ribbon's tabs: Object, Support and Slice edition, each with its own buttons; Esc puts the current tool away; Delete removes what is selected in the current mode.

### Set my preferences
**As a** maker, **I want** a Preferences window for the mouse mapping, zoom and orbit behaviour, language and import defaults (lift, arrange), **so that** the app works the way I do.
- Saved within a second of a change; a setting that doesn't read falls back to its default alone and the status bar says which.

**Not yet**
- **As a** French-speaking maker, **I want** the app in French.
- **As a** maker on any computer, **I want** to use ResinSupport in a browser.
