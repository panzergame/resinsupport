---
name: resinsupport-running
description: "Use when running ResinSupport to see a change working: launching it with a model, driving it with clicks and keys under Xvfb, and taking screenshots to check the UI."
---

# Running the app and looking at it

ResinSupport has no control channel yet; drive the real window instead.

## Launch
- `cargo run -p resinsupport -- path/to/model.stl` opens the window with the model imported and framed.
- Without a display, run it under Xvfb on the software renderer:
  `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json xvfb-run -a --server-args="-screen 0 1920x1000x24" <script>`
- A debug build on llvmpipe needs about 20 s before its first frame: wait before the first screenshot.

## Drive it (inside the script run by xvfb-run)
- Find and place the window: `WID=$(xdotool search --name ResinSupport | head -1)`, then `xdotool windowmove $WID 0 0` and `xdotool windowfocus --sync $WID`. eframe restores the size and position saved by an earlier run (possibly on a second monitor, off this screen), and egui restores its windows' positions too.
- Click with a pause after the move: `xdotool mousemove X Y`, `sleep 2`, `xdotool mousedown 1`, `sleep 0.1`, `xdotool mouseup 1`. A move and a press in the same instant can miss (egui picks where the pointer was the frame before), and a press held over 0.8 s is a long press, not a click: buttons ignore it.
- After the last input, move the pointer once more before the screenshot: egui repaints on input, and a new window shows only from its second frame.
- Keys: `xdotool key t` (shortcuts need the window focused and no text field being typed in).
- Screenshot: `import -window root -crop 1920x1000+0+0 shot.png`, then read the PNG.

## What to check
After a UI change, look at both the changed piece and its neighbours: the ribbon's dimmed and pressed states, a card's header and footer, the gizmo over a selected model, the status bar. Say in the report what was checked on screen and what was not.
