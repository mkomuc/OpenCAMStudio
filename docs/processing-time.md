# Processing time

Select the machine in **Machinery**, enter its **Rapid rate (mm/min)** and
**Tool change time (s)**, then apply the machine settings. These settings belong
to the local machine library.

Right-click a setup or operation and choose **Calculate processing time…**.
The setup estimate includes its enabled machining operations; an operation
estimate runs that operation in isolation. Missing paths are generated through
the normal background calculation flow.

The result shows feed travel and time, rapid travel and time, dwell time,
tool change count and time, and total processing time. Feed override changes
feed motion time only; it does not change the machining document.

## Estimate assumptions

- Motion time is distance divided by the programmed feed or machine rapid rate.
  Arcs use their circular length, including the Z travel of a helix.
- Drilling uses full retract between pecks with a 0.5 mm re-entry clearance,
  matching the app's expanded drilling moves. Controller cycle behavior can vary.
- Every generated tool change command incurs the configured delay, including
  the initial tool load. The default delay is zero until configured.
- The machine's initial position is unknown, so travel to the first known
  position is omitted. Work-coordinate changes also discard the previous
  position because their physical offset is unknown.
- Acceleration, controller look-ahead, spindle startup, and operator waits at
  program stops are not included. This is a nominal estimate, not a measured
  machine cycle time.
