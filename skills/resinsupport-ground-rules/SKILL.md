---
name: resinsupport-ground-rules
description: "Use before adding any dependency, icon, image, font, test reference data or product wording to ResinSupport, before porting code from another project, or when unsure whether something may be copied, read or named. Covers independence from other slicers, GPL code, SolidCraft ports, asset attribution and units."
---

# Independence, licences, assets and units

The rules that cannot be broken. `AGENTS.md` lists them in one line each.

## Independent
- Behaviour comes from public documentation, general slicer conventions and user feedback. Printer file formats come from public specifications only.
- Never copy another slicer's code, icons, artwork, help text, tooltips, templates or profiles (Chitubox, Lychee, PrusaSlicer, Cura, …).
- Do not drive, script or extract data from another slicer for this project: no test data, measurements, command lists, ids or files produced with them.
- Command ids are our own: dotted, lower-case `<area>.<thing>[.<variant>]`.

## No GPL, LGPL or AGPL code
- Do not read, copy or link PrusaSlicer, Cura, CuraEngine or UVtools. Do not even read their sources.
- Permissive dependencies are fine; say which one you add and why. Every crate below the apps must stay wasm-friendly (`skills/resinsupport-performance/SKILL.md`).

## Code and assets from SolidCraft
- SolidCraft (MIT OR Apache-2.0) is the reference for conventions, UI and geometry. Code ported from it keeps a header comment naming its source file and commit (`4bdffef0` so far), and gets a row in `ATTRIBUTION.md`.
- Port interaction (gizmos, navigation, dialogs) from SolidCraft rather than imitating another slicer.
- Every file asset (icon, image, font) is original or openly licensed and has a row in `ATTRIBUTION.md`. UI icons are SolidCraft's SVG masters, copied unchanged into `crates/ui-egui/icons` (`skills/resinsupport-ui/SKILL.md`).

## Units and the name
- Model units are millimetres, Z up; angles in commands are degrees.
- ResinSupport everywhere: crates `resin-*`, the binary `resinsupport`. The product name is not a translated message.
