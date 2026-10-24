# Selecting entry and exit positions

Open an operation’s **Lead-in and lead-out moves** tab. Under **Positions**, use
**Select** next to **Lead-in position** or **Lead-out position**, then click the
model. Entry and exit are independent. Blue markers identify entry preferences;
orange markers identify exit preferences. **Clear** restores automatic placement,
and Escape cancels an active selection. **OK** saves the operation’s changes.

Positions are stored in setup XY and remain preferences: tool radius, boundaries,
islands and holding tabs determine the eligible toolpath point. Closed contours
are cut completely before continuing to the selected exit. An exit near a holding
tab resolves to a gap, and tabs remain protected during the additional travel.

The same controls are available for **3D Adaptive Clearing**. A model point is
resolved to a safe cutter-centre entry near the selection. Adaptive exit travel
retraces the final connected cleared path before retracting; it does not shortcut
across islands or disconnected regions. Existing layer safety checks also cover
this exit travel. Older projects keep automatic placement until positions are set.
