# Processing time calculation

## Implementation plan

The feature estimates machining time for one operation or a complete setup.
The source requirements are in `docs/features/processing_time.md`; its reference
image provides the breakdown and feed override control.

1. Add a controller-neutral estimator to `cam-cldata`. Calculate feed and rapid
   travel separately, including circular and helical moves, drilling cycles,
   peck re-entry, dwell, and tool change commands. Reject invalid numeric inputs.
2. Store tool change seconds on each machine with a zero default for existing
   files. Expose both this value and the existing rapid rate in Machinery.
3. Add a **Calculate processing time…** context command. Use the existing scoped
   background job and cached operation fragments, accepting only its matching,
   completed result. Present distances, times, tool changes, and a total in a
   dialog with an adjustable feed override.
4. Check estimator arithmetic, machine-file compatibility and validation, and
   scoped GUI commands. Compile the desktop GUI to check integration.

## Usage

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

## Validation

The following checks passed during implementation:

- `cargo check --workspace --all-targets`
- `cargo test -p cam-cldata`
- `cargo test -p cam-model`
- `cargo test -p cam-app machines:: --lib`
- `cargo check -p cam-app --features gui`
- `cargo test -p cam-app --features gui processing_time`

Coverage includes peck drilling compared with explicit moves, helical full
circles, datum changes, numeric overflow, legacy machine files, feed overrides,
operation versus setup scope, and cancellation. Desktop interaction has not
been checked manually.
