# Selecting entry and exit positions

Open an operation’s **Lead-in and lead-out moves** tab. Its controls are grouped
in collapsible **Linking moves**, **Leads and transitions**, **Ramping**, and
**Positions** sections, as applicable to the strategy. Hover over a parameter or
strategy label for help (when tooltips are enabled).

Under **Positions**, use
**Select** next to **Lead-in position** or **Lead-out position**, then click the
model. The cursor snaps to enabled endpoints, midpoints, quadrants and nearest
edge points using the same controls as workpiece-origin selection. A live cross
previews the point; the selected marker stays on the clicked surface. Entry and
exit are independent. Blue markers identify entry preferences;
orange markers identify exit preferences. **×** clears the selected point and restores automatic placement,
and Escape cancels an active selection. The button reads **Point** once a point
is selected, and is highlighted while selecting. **Apply changes** saves the operation’s changes.

Positions are stored in setup XY and remain preferences: tool radius, boundaries,
islands and holding tabs determine the eligible toolpath point. Closed contours
are cut completely before continuing to the selected exit. An exit near a holding
tab resolves to a gap, and tabs remain protected during the additional travel.

The same controls are available for **3D Adaptive Clearing**. A model point is
resolved to a safe cutter-centre entry near the selection. Adaptive exit travel
retraces the final connected cleared path before retracting; it does not shortcut
across islands or disconnected regions. Existing layer safety checks also cover
this exit travel. Older projects keep automatic placement until positions are set.
