# Cutting forces per operation

The operation inspector estimates nominal engagement. For loads based on the
remaining stock, use [force mode in simulation](#force-mode-from-simulated-removal).

1. Open **Tools → Materials** and enter the material's **Specific cutting force kc
   (N/mm²)**. Use an effective coefficient appropriate to the material, chip
   thickness and cutter geometry. Blank means unknown; no default is assumed.
2. Select this managed material in **Setup → Stock**.
3. Open an operation's **Tool** tab and expand **Cutting forces**. The section
   shows mean tangential force in N, net cutting power in W, spindle torque in
   N·m, and removal rate in cm³/min. No generated toolpath or simulation is needed.

Estimates update with draft operation values, the staged cutter, resolved height
references, and the current material coefficient. Operation RPM 0 inherits the
job's spindle speed. Editing a material coefficient changes analysis only; it
does not rewrite feeds, presets, toolpaths or G-code.

## Engagement assumptions

Automatic estimates cover square/roughing end mills and face mills:

| Operation | Axial engagement ap | Radial engagement ae |
| --- | --- | --- |
| Profile | Smaller of stepdown and resolved cutting depth | Radial stepover, capped at tool diameter; full-width slotting when stepover is zero |
| Pocket | Smaller of stepdown and resolved cutting depth | Adaptive engagement cap when enabled; otherwise diameter × (1 − overlap) |
| Face | Smaller of stepdown and resolved cutting depth | Diameter × (1 − overlap) |
| 3D Adaptive | Smaller of roughing stepdown and resolved cutting depth | Optimal load, capped at tool diameter |

Resolved cutting depth means top height minus bottom height, including reference
and offset edits. These are nominal roughing or facing pass assumptions. Entries,
corners, finishing passes, shallow layers, spring passes and previously removed
stock can have different loads. A profile without radial roughing assumes
slotting; change the engagement for a known side cut.

Enable **Custom engagement** to enter axial engagement, radial engagement and
effective cutting diameter. Radial engagement cannot exceed the effective
diameter, which cannot exceed the cutter's nominal diameter. This also provides
a calculator for bore, thread, chamfer, engraving, carving and parallel milling,
or shaped cutters for which
contact cannot be inferred from operation parameters alone. Supply an equivalent
rectangular removed section and appropriate effective diameter; nominal tool
diameter can be inappropriate for a V-bit or ball nose. Inputs are per operation,
retained for this session, and cleared when opening or creating a project. They
are analysis inputs and are not saved or used to generate toolpaths. For a
multi-tool carve, the calculator describes the primary tool at its cutting feed;
it does not combine the separate clearing cutter's load.

Drilling thrust is unavailable: it requires a separate drill-specific model.
Missing material data, invalid feeds/speeds, invalid engagement and overflow
produce an explanation rather than a zero or fabricated force.

## Calculation

The metric equations use effective kc in N/mm², feed vf in mm/min, diameter D,
axial depth ap and radial width ae in mm, and speed n in rpm:

```text
Q  = ap × ae × vf                   [mm³/min]
vc = π × D × n / 1000              [m/min]
Pc = Q × kc / 60000                [W]
Fc = 60 × Pc / vc                  [N]
Mc = Fc × D / 2000                 [N·m]
```

Power and speed follow [Sandvik Coromant's metric milling
formulas](https://cdn.sandvik.coromant.com/files/sitecollectiondocuments/services/metal-cutting-e-learning/formulas-and-definitions/formulas-and-deinitions-for-milling-metric-enu.pdf).
The equivalent tangential force is derived from power divided by cutting speed;
torque uses the effective cutting radius. Enter effective kc, rather than an
uncorrected kc1 reference value: [specific cutting
force](https://www.sandvik.coromant.com/en-gb/knowledge/materials/specific-cutting-force)
depends on chip thickness and rake geometry.

The reported force is a revolution-average equivalent during the assumed steady
cut, not an operation-time average or a peak tooth force. The model does not
predict radial/axial components, transient peaks, helix effects, tool wear,
deflection, chatter or machine losses, and does not measure actual stock contact.
It does not compare loads against machine limits or alter export eligibility.

## Force mode from simulated removal

1. Set the stock material's kc as described above, generate the toolpath, then
   right-click the setup or operation and choose **Simulate toolpath**.
2. To color the cut faces, choose **Display → Stock → Coloring → Cutting force**.
   To color the toolpath, independently choose **Toolpath → Color by → Cutting
   force (actual removal)**. The stock mode works with the toolpath hidden.
   Analysis runs in the background; playback and seeking remain available.
3. Read the transparent gradient at the top right of the 3D viewport, to the left
   of the orientation cube, labeled in newtons (N). Blue represents air cuts or
   low loads, and red
   represents the highest sampled load in this program. Gray represents missing
   material/tool/spindle data, drilling, or motion after a detected collision.
   Rapids and other non-cutting links retain their normal move colors.
4. **Info** shows equivalent tangential force, net power, spindle torque and
   removal rate at the rendered tool position. **Statistics** shows removed
   volume and mean/max sampled force for each operation in the simulated program.

In stock mode, each cut face is associated with the move that exposed it and
that move's local force sample. Flat floors, walls from earlier depth passes,
and bounded thread flanks retain their cutting history. A later zero-removal
pass does not repaint those faces blue. Original stock retains its material
appearance; surfaces without force data are gray. The stock and path use the
same gradient and numeric scale, and viewport orbit/picking remains available
through the legend overlay.

This mode measures the volume newly removed from the evolving simulated blank
in each spatial interval. Earlier operations, overlapping cuts, repeated passes,
tool changes, ramps and helical arcs all contribute to that evolving stock.
Inherited stock is calculated from preceding setups in the destination setup's
axes. A spring pass through already cleared material has zero removal; a partial
side cut removes only the remaining portion. The simulated program's scope
determines which preceding operations are included in the current setup.

For a removed volume dV in mm³ over a programmed interval dt in seconds:

```text
Q  = 60 × dV / dt                  [mm³/min]
Pc = kc × dV / (1000 × dt)         [W]
Mc = 60 × Pc / (2π × n)           [N·m]
Fc = 60000 × Pc / (π × D × n)     [N]
```

Power and torque use measured removal and the programmed spindle speed. Force
is an equivalent tangential load referred to the active cutter's **nominal
diameter D**. For a V-bit, ball nose or other shaped cutter, this is not a local
flank/tooth force: contact radius varies. Bounded thread-tooth cuts retain and
subtract previously removed voids. Multi-tool operations use the cutter and RPM
actually commanded in each portion of the program.

Both removal and spatial resolution are approximate: the calculation uses the
same grid and cutter profiles as stock simulation, with intervals at least four
grid cells or half a tool radius long. Short moves use their own interval. Raising
**Stock → Accuracy** recalculates the trace; changing kc rescales loads without
resimulating stock. The color scale uses the whole analyzed program and stays
fixed while seeking, including All/Leading/Trailing/Current operation/Tail modes.
The chronological trace remains the same when rewind is configured to retain a
later stock mesh. Playback speed changes visualization only, not machining feeds
or the computed force.

The per-operation mean is weighted by time over sampled feed motions, including
air cutting, and excludes rapid/dwell time. Max is the largest **interval mean**,
not an instantaneous tooth-force peak. Incomplete force coverage is marked in
Statistics. Drilling is simulated to update stock for later cuts, but no drilling
force is reported. After collision contact, later loads are unavailable.

Analysis uses a separate cancellable worker and never changes verification,
toolpaths, machining parameters or export eligibility. Traces are session data;
the color-mode preference is saved. A 500,000-sample limit bounds analysis memory.
Forces remain estimates using effective kc; chatter, deflection, acceleration,
wear, radial/axial force components and spindle losses are not modeled.

## Saved data and automation

Tool-library format v4 adds optional `specific_cutting_force` on each material.
Older libraries load with it unset. A library export preserves coefficients;
project files continue to reference the managed material by ID. On a different
computer, import the material library to make its coefficients available for
analysis. No project schema change is required.

MCP offers `set_material_cutting_force` (omit the coefficient to clear it) and
`get_cutting_forces` for an operation in the active setup. The latter accepts an
optional `engagement` object with `axial_depth_mm`, `radial_width_mm`, and
`cutting_diameter_mm`, and returns the estimate, material, assumptions and limits.
It is read-only and does not require a completed run. Persist shop changes with
`save_shop` or `export_tool_library`.
