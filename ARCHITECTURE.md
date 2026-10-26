# Project overview

OpenCAMStudio turns CAD geometry into CNC milling programs:

**Import → Setup and stock → Operations and tools → Toolpaths → Simulation → G-code**

A setup describes one machining orientation, its work origin and stock. Operations
choose what to cut and how. The same calculated toolpaths feed the preview,
material-removal simulation and controller-specific G-code export. Machine settings
describe physical limits; the selected post determines the controller dialect.

Drawing-based work covers 2.5D milling. STEP/IGES workflows add linked feature
selection and conservative three-axis roughing. Freeform finishing, simultaneous
multi-axis machining, turning and third-party plugins remain future work.

Start with the [user guides](docs/README.md) for workflows and current limits,
or the [release notes](docs/release-notes/v0.1.0.md) for release history.

Crate dependency boundaries and optional GUI dependencies are enforced by
[`scripts/check_architecture.py`](scripts/check_architecture.py) in CI. Behavioral
contracts are checked by the Rust tests; crate responsibilities and APIs live
with their source rather than in a separate architecture specification.
