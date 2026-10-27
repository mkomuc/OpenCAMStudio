# Geometry selection and advisory verification

Automatic feature verification is advisory. A wall, access, face-orientation or
cutter-side recognition failure is information; it does not veto a usable
selection. The selected curves and the operation's Side and Heights settings
define the cut when recognition cannot infer those settings. The original
recognition routines remain available through `verify_profile_entry` and
`verify_selection` for analysis and future policy changes.

In Chain settings, **Select individual edges** is independent of **Closed/Open**.
Switching chain mode keeps the selected edges and the picking granularity.
Connected edges can form a closed contour without an adjoining upward face.
Direct profile selections group connected edges into one entry, while different
feature kinds and body instances become separate entries.

Geometry extraction still needs usable input. Missing/stale references,
unsupported exact curves, disconnected or branching paths and degenerate
boundaries are geometry-data errors. Picks remain available for correction;
an incomplete closed chain can be assembled without opening an error dialog on
every pick. Toolpath generation needs a computable path and valid numeric
settings. Collision and NC-export checks continue to run.

## Local verification observations

Selection changes, geometry configuration and operation creation record local
JSONL observations. Records append across projects and application sessions.
No network service is involved. The default file is:

- macOS: `~/Library/Application Support/OpenCAMStudio/geometry-verification.jsonl`
- Linux: `$XDG_CONFIG_HOME/OpenCAMStudio/geometry-verification.jsonl`, or
  `~/.config/OpenCAMStudio/geometry-verification.jsonl`
- Windows: `%APPDATA%\OpenCAMStudio\geometry-verification.jsonl`

`OPENCAM_GEOMETRY_VERIFICATION_LOG` overrides the path, including for tests.
An unwritable log is reported without rejecting the operation's geometry.

Each line has `timestamp_unix_ms` and an `event` containing:

- schema, verifier and application versions, and `policy: "advisory"`;
- project/source names, source fingerprints and import tolerances;
- setup coordinates, stock, selected entity references and placements;
- selected exact curves/loops and adjacent surface descriptions and bounds;
- operation kind and entry settings, including chain mode, side and tolerance,
  and the geometry binding/heights when committing an operation;
- `selection_accepted`, `geometry_resolved`, `verification_passed`, a diagnostic
  category, the original verification message and any geometry-extraction error.

`verification_passed: false` with `geometry_resolved: true` records a usable
selection that the automatic classifier did not verify. This distinction lets
later analysis compare successful user selections with the old classifier's
findings, refine recognition, and evaluate operation suggestions or an optional
enforcement policy.

Full source-file bytes and display/projection meshes stay in the project. For a
selection whose detailed control nets exceed 256 KiB, the record retains its
identities and context and marks the geometry details as omitted. Repeated
identical observations are suppressed within a running process; drawing a frame
does not write a log record.

The manual `supplied_test2_eight_edges_create_a_closed_chain_with_advisory_verification`
test qualifies the eight-edge selection from `test2.ocam`, including switching
modes, operation creation and save/reopen.
