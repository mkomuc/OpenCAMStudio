# Simulation

Right-click a setup or operation and choose **Simulate toolpath**. The simulation panel
opens beside the viewport. Use the playback controls to play, pause, change speed,
seek, or step between operations and motion points. **Close simulation** returns
to the machining inspector. Display preferences are saved between sessions.

The **Display** tab has four collapsible sections:

- **Tool**: show the cutter, cutter with shaft, or cutter with a schematic holder;
  enable transparency or mark the programmed tool tip. Holder dimensions are
  illustrative; the holder is not included in collision checking.
- **Toolpath**: independently show links, entry/exit moves and retracts, cutting
  moves, and motion points. **All toolpaths** shows the entire simulated program;
  **Trailing** shows completed motion; **Leading** shows remaining motion;
  **Current operation** shows the complete active operation; **Tail** shows the
  last 20 mm traveled. Partial linear moves, arcs, helices, and drilling pecks
  follow the rendered tool position.
- **Stock**: show or hide material, choose coloring and material appearance,
  adjust simulation accuracy, or enable transparency. Higher accuracy resolves
  finer cuts and costs more time and memory. Changes rebuild the playback stock
  when the accuracy slider is released and apply to subsequent calculations.
  With **Regenerate stock on rewind** enabled, seeking backward restores earlier
  material. With it disabled, the stock retains the furthest simulated cut while
  the tool and path follow the timeline. **Stop on collision** pauses at the
  first unacknowledged stock contact and highlights the tool red; Play continues
  past that contact. Disabling it allows playback through detected contacts.
- **Display**: show or hide the imported model or drawing.

Stock coloring supports **Material**, **Operation**, **Tool**, and **Comparison**.
Operation and tool colors identify which operation or tool exposed a surface;
original stock faces keep the selected material appearance. Comparison uses the
closed CAD solids assigned to the active setup: blue indicates excess material,
green indicates a surface within the chosen tolerance, and red indicates a gouge.
It compares stock vertices to the imported CAD tessellation, so its precision is
limited by that tessellation and the simulation grid. Without a closed target
solid, comparison surfaces are gray and the panel explains why.

The **Info** tab shows the rendered tool, operation, coordinates and collision
message. **Statistics** shows duration, rendered time, motion count, stock triangle
count and grid spacing. Playback uses a separate simulation lane and does not
replace the verified result used for G-code export.
