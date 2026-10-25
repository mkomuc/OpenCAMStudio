# Thread geometry and standard-size presets

Thread's **Geometry** tab contains a **Thread** section with sizing mode,
series, standard size, major diameter, pitch and hand. Pitch and hand are also
available in **Passes**; both tabs edit the same operation values.

Choose a standard size to set nominal major diameter and pitch together. This
preserves the configured hand, internal/external type, heights, feeds, cutter,
cut direction and pass settings. Choose Custom or type either dimension to
return to Manual. Inch presets display their designation and TPI while storing
pitch as `25.4 / TPI` mm, without rounding that conversion in the edit buffer.

## Matching the selected geometry

New Thread drafts start in **Match prepared bore** mode. Existing operations
open in **Manual** so revisiting an operation cannot alter its saved cut. Enable
either matching mode to size it from its current selection:

- **Match prepared bore:** an internal cylinder or drawing circle represents
  the prepared hole. Compare it to the basic minor diameter and approximate
  cut-thread preparation size. Examples: 5 mm → M6 × 1; 6.8 mm → M8 × 1.25;
  30 mm → M32 × 2 (with other close standards available).
- **Match nominal diameter:** the selected cylinder represents the nominal
  thread envelope. Match major diameter instead. A nominal 30 mm cylinder
  suggests M30 coarse, with its fine pitches offered as alternatives.
- External bosses always match nominal major diameter.

The series selector restricts matching and the standard-size list to Metric
coarse, Metric fine, UNC or UNF. All series are available by default. Close
candidates appear before the rest of that series' catalog. The closest match
is applied automatically while matching is enabled. Equal-distance ties prefer
metric coarse, metric fine, UNC, then UNF; alternatives remain selectable.
Manually selecting a preset stops automatic replacement of that choice.

The displayed source diameter comes from selected CAD geometry or drawing
circles, independently of the operation's editable major diameter. If no close
candidate is found, parameters stay unchanged and an explanatory message appears.
Mixed source diameters and stale CAD references cannot provide a match. Geometry subdialog selections are not
applied until confirmed; matching an unconfirmed replacement cannot change the
operation's parameters.

The catalog covers common metric coarse sizes M1–M64, selected metric fine
diameter/pitch combinations in that range, and Unified numbered/fractional sizes
#0–1½ inch where the chosen UNC/UNF series exists. It is a finite catalog,
not every size in the governing standards; other sizes remain editable manually.

## Meaning of a standard preset

Presets supply standard size/pitch pairs for the strategy's cylindrical 60°
form. They do not select a tolerance class, fit allowance, thread engagement,
material strength, blind-hole clearance or a cutter. Prepared-bore matching is
a proximity suggestion, not a thread-identity or tolerance certificate.
Cut-thread preparation hints use published common drill sizes where listed,
otherwise metric `D − P` or Unified's approximate 75% cut-thread diameter.
The existing tool-fit, neck/reach and geometry checks still apply. A nominal CAD
envelope and a physically prepared bore are different matching inputs.

British Whitworth/BSP, pipe/taper, trapezoidal and other thread forms are not
included because the current thread strategy does not implement their geometry.

Catalog references:

- [Guhring thread-milling technical catalog](https://guhring.com/Catalogs/CatalogPDF/cd2235c8-4be9-44e4-8570-cd952bd6b97a): metric coarse/fine size/pitch pairs and common preparation diameters.
- [OSG cut-tap and pitch-diameter reference](https://res.cloudinary.com/osg-usa-inc/image/upload/v1717093846/Literature/03%20-%20Charts%20and%20Guides/OSG_-_Chart_-_Tap_Drill_Size_and_Pitch_Diameter_Limit_-_Vol_3_-_2024_-_IA.pdf): Unified UNC/UNF size/TPI pairs and cut-thread preparation formula.
- [HEICO technical tables](https://heicobolting.com/technical-info/torque-charts/): large metric coarse diameter/pitch combinations.
