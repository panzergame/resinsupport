---
name: resinsupport-ground-rules
description: "Use before adding any dependency, icon, image, font, test reference data or product wording to ResinSupport, before porting code from another project, or when unsure whether something may be copied, read or named. Covers other slicers, the GPL licence, SolidCraft ports, asset attribution and units."
---

# Other slicers, licences, assets and units

The rules that cannot be broken. `AGENTS.md` lists them in one line each.

## Other slicers
- Open-source slicers and tools under a GPLv3-compatible licence (mslicer GPL-3.0, Cura LGPL-3.0, …) may be read and ported: code, file formats, profiles, icons, text. Each port keeps a header naming its source file and commit and gets a row in `ATTRIBUTION.md`. AGPL ones (UVtools, PrusaSlicer, CuraEngine) follow the AGPL rule below.
- Printer file formats come from public specifications or from such an implementation; name the document or source file followed in the format file's header.
- Files produced by an open-source tool may be test reference data; say which tool and version made them beside the file.
- Proprietary slicers (Chitubox, Lychee, …) stay out: never copy their code, icons, artwork, help text, tooltips, templates or profiles, and never drive, script or extract data from them (no test data, measurements, command lists, ids or files produced with them).
- Command ids are our own: dotted, lower-case `<area>.<thing>[.<variant>]`.

## Licence: GPL-3.0-or-later
- ResinSupport is GPL-3.0-or-later (`LICENSE`). Releases before the switch stay MIT OR Apache-2.0.
- Code and dependencies taken in must be GPLv3-compatible: permissive (MIT, Apache-2.0, BSD, Zlib), LGPL-3.0, GPL-3.0(-or-later). GPL-2.0-only is not.
- AGPL-3.0 code from UVtools may be ported (the owner allowed it): ported files stay AGPL-3.0 and say so in their header, their `ATTRIBUTION.md` row names AGPL-3.0, and a served web build must offer its complete source. Other AGPL projects (PrusaSlicer, CuraEngine) still need the owner's say.
- Code taken from a GPL or LGPL project keeps a header naming its source file and commit, and gets a row in `ATTRIBUTION.md`, as SolidCraft ports do.
- Say which dependency you add and why. Every crate below the apps must stay wasm-friendly (`skills/resinsupport-performance/SKILL.md`).

## Code and assets from SolidCraft
- SolidCraft (MIT OR Apache-2.0) is the reference for conventions, UI and geometry. Code ported from it keeps a header comment naming its source file and commit (`4bdffef0` so far), and gets a row in `ATTRIBUTION.md`.
- Port interaction (gizmos, navigation, dialogs) from SolidCraft rather than imitating another slicer.
- Every file asset (icon, image, font) is original or openly licensed and has a row in `ATTRIBUTION.md`. UI icons are SolidCraft's SVG masters, copied unchanged into `crates/ui-egui/icons` (`skills/resinsupport-ui/SKILL.md`).

## Units and the name
- Model units are millimetres, Z up; angles in commands are degrees.
- ResinSupport everywhere: crates `resin-*`, the binary `resinsupport`. The product name is not a translated message.
