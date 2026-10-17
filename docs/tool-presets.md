# Tool presets and stock materials

The **Tools** inspector contains **Cutting presets** for the selected cutter and a
shared **Materials** list. Materials currently contain only a stable ID and a name.
Rename a material without changing its ID; presets and stock use the ID to match.
Materials are saved with the cross-project tool library and included in library
imports and exports.

1. Add materials under **Tools → Materials**.
2. Select a cutter and use **Add / duplicate** under **Cutting presets**. With a
   preset selected this copies its values. Give the new preset a name and select
   its material, or leave it at **Any material**.
3. Enter spindle RPM, cutting feed, and plunge feed. Optionally enter stepdown and
   overlap. Click **Apply changes** to save the cutter and presets together.
4. In **Setup → Stock**, choose the stock material from the managed list.
5. Create an operation and choose a cutter. Its first preset matching the stock
   material is applied automatically. If none matches, its first **Any material**
   preset is used. If neither exists, the existing nominal/job defaults apply.

The operation's **Tool** tab lists every preset for that cutter, including presets
for other materials. Choose another to apply its values, or edit the operation's
cutting parameters directly. Stepdown and overlap are also available on this tab
where the operation supports them. Presets affect cutting data only; tool geometry
continues to be defined by the cutter.

Stepdown maps to axial depth per pass for milling/engraving and peck depth for
drilling. Drilling uses the plunge feed as its axial feed. Chamfer width increments,
thread milling, and the primary V-carve pass do not use the axial stepdown preset;
overlap applies to pocketing and facing. Other strategy settings remain editable
in their existing tabs.

Changing presets replaces the supported cutting values; subsequent manual edits
belong to that operation and do not update the library. Selecting **Manual cutting
data** removes the preset association while retaining the current values. Cancel
and undo work with preset selection and parameter changes together.

Projects embed their tools, available presets, selected preset baseline, material
ID/name, and concrete operation values. Opening a saved project does not reapply
library presets. To use updated library data, explicitly select the cutter again.
Changing stock material does not rewrite existing operations. Older files without
presets or managed material IDs retain their existing cutting data and material name.

A material used by a library preset or stock in the current project must be
unlinked before it can be deleted. Saved projects remain self-contained when a
material is unavailable in the local library.
