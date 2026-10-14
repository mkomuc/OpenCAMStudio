# Tool library software implementation reference

Autodesk Fusion screen recording analysis

14 October 2026

## 1 Purpose and implementation scope

The recording demonstrates a tool library inside Autodesk Fusion's Manufacture workspace. Its central function is to manage reusable manufacturing tool definitions, their physical geometry, assembly information, cutting-data presets, and NC output metadata. The demonstrated journey creates a document tool, defines a shaft segment, adds an alternative cutting preset, recovers from expression errors, and saves the result.

Build the new implementation around a tool definition with independently editable metadata, typed geometry, optional assembly components, multiple cutting presets, and postprocessor settings. A library browser coordinates these records across document, local, cloud, built-in, and supplier sources. The UI needs a dependency-aware parameter engine: changing spindle speed updates related quantities, and invalid expressions prevent committing the entire tool.

This reference covers every readable tool type, field, panel, menu option, and interaction identified in the recording. It does not claim that every type's configuration was demonstrated. Only the flat end mill editor is exercised. Architecture, persistence rules, API contracts, unshown type-specific parameters, and acceptance criteria are designs for a new implementation.

The screen recording is the primary behavioral source. Official Autodesk documentation is used only for limited terminology and explicitly identified supplementary capabilities. The reference describes software behavior; the example cutting values are test fixtures, not a machining recommendation.

## 2 Evidence and interpretation rules

### Evidence labels

| Label | Meaning | How developers should use it |
| --- | --- | --- |
| O behavior | An interaction and its visible result are demonstrated | Reproduce the result in the relevant workflow |
| O control | A readable control, field, menu entry, or static state is visible | Include the capability in scope; do not assume unshown semantics |
| I | Inference from visible structure or behavior | Confirm the assumption or adopt it explicitly |
| P | Proposed design for the new implementation | Treat it as a recommendation, not a claim about Fusion |
| D | Supplementary official documentation | Keep it separate from recording evidence |
| U | Unknown from the recording | Resolve before claiming complete parity |

References such as E07 point to the evidence ledger below. All timestamps are elapsed time from the local recording, not from the earlier YouTube URL. Times are approximate to the nearest second; the decoder sometimes returns the nearest available frame during static intervals.

### Source and inspection coverage

The source is Bildschirmaufnahme 2026-10-14 um 13.13.35.mov, a 144.203-second recording at 1666 by 1134 pixels. The matching local Desktop copy was inspected. Its SHA-256 is 7dfa0105d6b9f388a00a8b6fd50049defd81a5ac916cdb89cb69be39546c53fd. Media inspection identified one video track and no audio track.

The visual review covers the full 00:00–02:24 duration: 49 frames sampled at three-second intervals, then 145 samples at one-second intervals. Contact sheets were reviewed across all intervals, followed by native-resolution inspection of key forms, menus, tooltips, errors, and saved records. This method supports the listed visible features but cannot guarantee capture of every subsecond cursor movement or transient popup.

The user clarified that the description “Schaftfräser 5mm” alongside a 6 mm diameter was a demonstration error. Geometry and numerical fixtures in this reference use 6 mm. The discrepancy is not an intended feature, validation rule, or requirement to reproduce a mislabeled record.

### Evidence ledger

| ID | Time | Visible evidence |
| --- | --- | --- |
| E01 | 00:00–00:02 | Library tree, grouped tool table, category filters, empty cutting-data panel |
| E02 | 00:03–00:10 | Selected existing tapered mill; Info panel with preview and properties; cutting preset row |
| E03 | 00:11–00:14 | New Tool chooser with six categories and 33 labeled creation tiles; flat end mill selected |
| E04 | 00:15–00:33 | General tab; description, vendor, product ID and link fields; vendor tooltip exposes parameter metadata |
| E05 | 00:34–00:46 | Tool tab; spindle direction, flute count, material menu, geometry; type dropdown also contains Formfräser |
| E06 | 00:47–01:05 | Geometry edits; dimension highlighting; final diameter 6 mm, total length 56 mm, flute length 20 mm |
| E07 | 01:06–01:11 | Empty Shaft tab followed by one added segment; height and upper/lower diameters 10 mm; preview changes |
| E08 | 01:12–01:13 | Holder tab with search, empty list, metadata and select/extract/remove controls |
| E09 | 01:14–01:19 | Cutting Data tab; default preset; new preset named Alu through inline name entry |
| E10 | 01:20–01:24 | Alu selected; spindle edit toward 24000 rpm; expression autocomplete appears |
| E11 | 01:25–01:31 | Invalid expression; inline and dependent errors; red tab markers, Problems tab, disabled OK |
| E12 | 01:32–01:41 | Valid spindle restored; derived values recalculate; cutting feed changed to 2000 mm/min |
| E13 | 01:42–01:50 | Nine-option coolant menu; mist selected; contextual illustrated plunge-feed tooltip |
| E14 | 01:52–01:56 | Postprocessor tab with numbering, offset indices, turret, comment and three boolean options |
| E15 | 01:57–02:12 | Tool saved into document group; two preset rows; success notification; close tooltip names Escape |
| E16 | 02:13–02:24 | Return to Fusion Manufacture viewport; wooden toy model, setup and operation tree, workspace toolbar |

## 3 Product structure and library browsing

### Main window layout

O control — E01, E02 and E15. The Werkzeugbibliothek window has three vertical regions. The left region holds search and a hierarchical source tree. The center holds the main tool table above a smaller cutting-data table. The right region switches between Filter and Info. A Close button sits at the bottom right, and a help icon appears near the top right. Different panels have their own scrollbars.

The main table is grouped by library or document. At the start, Tigerente v7 and Fräser are visible group headings. Rows display a tool icon and properties. The readable columns are Name, Eckenradius, Durchmesser, Schneidenlänge, Gesamtlänge, and Typ: name, corner radius, diameter, flute length, overall length, and tool type. Long names and headings are truncated. An upward sort indicator appears on diameter. A gear appears at the left of the column header. Disclosure triangles are present on some rows but are not expanded in the recording.

O control — The lower table shows preset name, spindle speed, surface speed, cutting feed, feed per tooth, lead-in feed, lead-out feed and coolant columns. The right Info table can also show type-specific values absent from the demonstrated flat-mill editor. For the selected tapered tool, corner radius is 1 mm and Winkel is 2.8 degrees. Its name summarizes tool number 29, diameter 2 mm, radius 1 mm and an angle displayed as 3 degrees. A blue product URL is displayed as a link, but it is not opened. Type, units, spindle direction and material appear as values such as tapered mill, millimeters, true and hss; localization of these stored values should be handled explicitly in the new implementation.

O behavior — E02 and E15. Selecting a tool highlights its row in pale blue and populates the lower preset table and right-side Info panel. Without a selection, the lower pane prompts Werkzeug auswählen. The selected tool's Info view contains a source breadcrumb, generated name, grid-based preview, and a scrollable property-value table. The saved tool appears in the active document group while the previously visible source records remain below it.

P — The browser should maintain a single selected tool ID, an optional selected preset ID, selected library scope, filter state, and sort state. Resolve all pane contents from these IDs. Clear stale detail panes if selection disappears after deletion or filtering. Preserve a selection after save if the tool remains in the result set.

### Library hierarchy

| Visible source | Visible children or examples | Evidence and boundary |
| --- | --- | --- |
| Benutzerbibliotheken | Documents, Hub Libraries, Cloud, Local, Fusion Library | O control E01; parent grouping |
| Dokumente | Tigerente v7, Setup1, Setup1 (2) | O control E01; tool creation into document demonstrated E15 |
| Hub Bibliotheken | Label and information icon | O control E01; no expansion or shared editing demonstrated |
| Cloud | Bohrer and Fräser | O control E01; synchronization behavior unshown |
| Lokal | Library | O control E01; file storage and editing here unshown |
| Fusion Bibliothek | Metric/inch turning and milling tools; holder variants; tutorial tools; cutting tools; probes; hole-making tools | O control E01; several labels are truncated |
| Zulieferer | BitsBits Cutting Tools, Blum Touch Probes, DATRON Cutting Tools; Delta Tools heading | O control E01; lower supplier entries extend beyond viewport |

O control — A checked Deaktivierte Bibliotheken anzeigen option is visible below the tree. Several built-in entries appear gray. The recording does not establish whether a particular gray item is disabled, unavailable, or read-only. P — represent enabled state, availability, and write permission separately rather than deriving them from text color.

I — Document and setup nodes imply project context for tools used in machining operations. The exact relationship between a setup node and the document's tool storage is not established. P — store document tool definitions once and maintain setup/operation references, unless an explicit product decision requires setup-specific copies.

### Search and filters

O control — The left search field is labeled Suchen. The right Filter tab shows a Werkzeugkategorie group with choices Fräsen, Bohren, Drehen, Schneiden, Antasten, and Halter. Filter löschen is visible above the tool table. No query is typed and no category filter is applied, so search fields, matching rules, scope and filter combinations are U.

P — Search name, description, vendor and product ID within the selected source scope; combine the query with one category filter and retain sort order. Show result count, loading and empty states. Clear Filters should clear active query/filter conditions according to a documented scope. Decide whether the left search filters the tree, tools, or both before freezing the UI contract.

### Toolbar inventory

O control — The main toolbar contains a plus, pencil, document icons, clipboard icon, overlapping-square icon, printer icon, a numeric mapping icon resembling 1→6 / 2→7, a broom-like icon, and a trash can. The plus opens the creation chooser, which is O behavior E03. Several controls are gray without a selection and blue after selection. Exact permissions and activation rules are not tested.

I — The remaining icons likely represent edit, copy, paste, duplicate, print, renumber, cleanup and delete actions. Their exact command names and effects are not established by the footage. The lower cutting-data toolbar contains pencil, copy-like, clipboard and delete icons. The editor preset toolbar has plus, pencil, paired up/down arrows and trash. P — provide explicit accessible names and tooltips, implement only validated semantics, and do not equate icon resemblance with a verified command.

D — Official preset documentation identifies the paired-arrow command as sorting presets. Thus manual move-up/move-down behavior must not be inferred from its appearance. The same documentation describes rename, delete, copy/paste and drag reordering; those actions are not demonstrated here. See Section 15.

### Surrounding Manufacture workspace

O control — E16. After the library closes, the host window title identifies Autodesk Fusion Personal and the Tigerente v7 document. The workspace selector reads Fertigen. A document tab, file/save-style controls and undo/redo-style arrows are visible. No menu is opened in this interval.

| Visible host region | Readable content | Evidence boundary |
| --- | --- | --- |
| Process tabs | Fräsen, Drehen, Additiv, Überprüfung, Fertigung, Dienstprogramme | O control; only milling is selected |
| Toolbar groups | Setup, 2D, 3D, Bohren, Aktionen, Verwalten, Prüfen, Auswählen | O control; dropdown commands remain unshown |
| Browser hierarchy | Tigerente v7, Einheiten mm, Benannte Ansichten, Ursprung, Modelle, Setups | O control; hierarchy and visibility-style icons |
| Setup nodes | Setup1 and Setup1 (2) | O control; no new setup is created |
| Existing operations | [T36] 2D-Kontur1, [T29] 2D-Kontur2, [T6] Bohrfräsen1 | O control; tool-number labels establish existing references |
| Expanded operation | Tool row for number 36, a 194.6 kB entry, Konturauswahl | O control; meaning of size entry is U |
| Model viewport | Wood-textured toy body and two wheels, edge outlines, grid, axes, orientation cube | O control; no machining/simulation result is demonstrated |

P — Treat this screen as the library's host integration context. The library should return to its caller and preserve document/setup context. The visible operation tree motivates tool references and revision handling, but it does not specify the host's full CAM command set. Green checkmarks and visibility-style icons are present; exact status transitions are untested.

## 4 Complete visible tool type catalog

O control — E03. The creation chooser contains 33 tiles. Each tile combines an icon and label under one of six category headings. At E05 the milling type dropdown exposes an additional Formfräser entry, making 34 distinct visible type labels across the recording. No configuration forms are shown for the other 33 types.

The English names below are working equivalents for implementation. Suggested geometry fields are P. They describe schema scope to investigate, not fields observed in the unvisited editors. Every type needs a stable internal discriminator, category, localized label, icon, parameter schema, validator and preview adapter. Operation compatibility must be explicit rather than inferred from category alone.

### Milling

| Visible German label | Working English equivalent | Proposed distinguishing parameters |
| --- | --- | --- |
| Kugelkopffräser | Ball end mill | Diameter; spherical tip profile; flute and overall length |
| Torusfräser | Bull nose or toroidal end mill | Diameter and corner radius |
| Schaftfräser | Flat end mill | Diameter, shaft diameter, lengths; demonstrated editor |
| Planfräser | Face mill | Cutting diameter; body profile; insert or tooth count |
| Konusfräser | Tapered mill | Tip diameter; taper angle; length; optional tip radius |
| Radienfräser | Radius or corner rounding cutter | Cutting radius and associated profile dimensions |
| Gravur-/Fasenfräser | Engraving or chamfer cutter | Tip diameter and included angle |
| Schwalbenschwanzfräser | Dovetail cutter | Head/neck diameters; angle; head length |
| Lollipop-Fräser | Lollipop cutter | Ball/head diameter; neck diameter; exposure |
| Nutenfräser | Slot mill | Disc diameter; cutting width; neck and shaft |
| Gewindefräser | Thread mill | Pitch; tooth profile; diameter and cutting length |
| Kreissegment Tonne | Circle segment barrel | Barrel profile radius and contact region |
| Kreissegment Linse | Circle segment lens | Lens profile radii and contact region |
| Kreissegment Oval | Circle segment oval | Oval profile dimensions and contact region |
| Kreissegment Verjüngung | Circle segment taper | Tapered segment profile and orientation |
| Formfräser | Form mill | Custom profile reference and dimensional envelope |

The first 15 milling labels are visible in the creation chooser; Formfräser is visible only in the editor type dropdown. U — the recording does not show how a form profile is created or imported, or whether this type is available in a different creation path. Do not assert a file format or custom-profile editor from this label alone.

### Drilling and hole making

| Visible German label | Working English equivalent | Proposed distinguishing parameters |
| --- | --- | --- |
| Bohrstange | Boring bar | Cutting envelope, tip location and boring reach |
| Zylindrische Senkung | Counterbore | Cutting/pilot diameters and pilot length |
| Bohrer | Drill | Diameter, flute length and point angle |
| Zentrierbohrer | Center drill | Pilot/tip dimensions and shoulder angle |
| NC-Anbohrer | Spot drill | Diameter and point angle |
| Reibahle | Reamer | Diameter, lead geometry and flute length |
| Konische Senkung | Countersink | Included angle and tip/cutting diameters |
| Linksgewinde | Left-hand tap | Diameter, pitch and left-hand thread direction |
| Rechtsgewinde | Right-hand tap | Diameter, pitch and right-hand thread direction |

### Turning

| Visible German label | Working English equivalent | Proposed distinguishing parameters |
| --- | --- | --- |
| Drehen - allgemein | General turning tool | Insert profile, nose radius, holder orientation |
| Drehen - Bohren | Turning boring tool | Insert and bar geometry; boring reach |
| Drehen - Einstechen | Turning grooving tool | Insert width and grooving envelope |
| Drehen - Gewinde | Turning threading tool | Thread profile, pitch range and insert orientation |

P — Turning schemas must represent non-axisymmetric insert placement and orientation. The revolved mill/shaft preview design cannot alone represent every turning assembly. No insert dialogs or orientation rules are observed.

### Cutting probing and holders

| Visible German label | Working English equivalent | Proposed schema scope |
| --- | --- | --- |
| Wasserstrahl | Waterjet tool | Process-specific nozzle/kerf metadata |
| Laserschneidwerkzeug | Laser cutting tool | Process-specific cutting-width and process metadata |
| Plasmaschneider | Plasma cutting tool | Process-specific kerf and process metadata |
| Antasten | Touch probe | Stylus tip radius, stem and assembly geometry |
| Halter | Holder | Metadata, gauge length and ordered geometry segments |

O control — Only category/type availability is established for these five items. Power, pressure, pierce delay, compensation, probe calibration, holder collision testing and machine execution are U. P — keep these schemas separate from mill feeds and flute counts; do not populate unshown controls merely because a category exists.

## 5 Tool editor and parameter inventory

### Editor shell and transaction controls

O control — E04–E14. The editor replaces the browser view with tabs Allgemein, Werkzeug, Schaft, Halter, Schnittdaten, and Postprozessor. A breadcrumb identifies Tigerente v7 and the draft's generated name. A persistent right-side preview remains visible across tabs. OK and Abbrechen sit at the bottom right.

O behavior — E06. Values and the generated title change while navigating between tabs. Draft data survives tab switches. E11 shows OK disabled during validation errors. E15 shows a committed tool, its presets, and a success notification after the editor closes. U — Cancel discard behavior, reopening after application restart, and save-to-disk timing are not demonstrated.

P — Use a draft transaction that encompasses metadata, geometry, shaft segments, holder assignment, all presets, and post settings. Tab navigation must never commit partial objects. Cancel discards the draft. OK validates the full aggregate and commits atomically. A failed commit keeps the draft and provides a retryable error without duplicating the tool.

### General tab

| Visible field | English meaning | Observed example and proposed data type |
| --- | --- | --- |
| Beschreibung | Description | Schaftfräser 5mm, user-confirmed demonstration error; string |
| Zulieferer | Vendor or supplier | Aliexpress; string |
| Produkt-ID | Product identifier | 567954; string, not numeric identity |
| Produkt-Link | Product link | Empty in demonstrated record; optional URL string |

O behavior — E04. Text is entered into the first three fields. The vendor tooltip shows a description, parameter name tool_vendor, current value and expression. P — store product IDs as text to preserve leading zeros and catalog punctuation. Description should be editable and should not control numerical geometry. If the product detects diameter text in a description, an optional nonblocking consistency suggestion may help users; do not make the demonstration error a required fixture.

O control — The product-link input exists; no link is entered or opened. P — validate a provided URL without fetching it automatically and render it as an explicit user-openable link in Info.

### Tool tab

| Field or group | Visible values or controls | Evidence and implementation note |
| --- | --- | --- |
| Typ | Schaftfräser plus milling type dropdown | O control E05; changing to another type is untested |
| Einheit | Millimeter selected; dropdown | O control E05; inch option not opened in this recording |
| Spindelrotation im Uhrzeigersinn | Checked checkbox | O control E05; clockwise flag |
| Anzahl der Schneiden | Spinner showing 3 | O behavior E05; positive integer flute count |
| Material | Nicht angegeben, HSS, Titan-beschichtet, Hartmetall, Keramik | O control E05; HSS retained |
| Durchmesser | Final 6 mm | O behavior E06; cutting diameter |
| Schaftdurchmesser | Final 6 mm with fx affordance | O control/behavior E06; linked numeric quantity |
| Gesamtlänge | Changed to 56 mm | O behavior E06; overall tool length |
| Länge unter Halter | 24 mm | O control E06; projection below holder |
| Schulterlänge | 21 mm | O control E06; shoulder length |
| Schneidenlänge | Changed from 18 to 20 mm | O behavior E06; flute length |
| Werkzeugbaugruppe Messlänge | 24 mm with fx affordance | O control E06; assembly gauge length |

O behavior — Geometry fields are selected and edited. The explanatory drawing beside them highlights corresponding dimensions with cyan arrows. The right preview displays a gray upper region and gold lower region on a grid. Their colors change spatial extent as parameters change. I — these regions distinguish cutting and noncutting geometry; the footage does not define a universal color legend for every type.

P — Keep the dropdown's material values for UI compatibility, but separate substrate from coating in an extensible internal model if future tool catalogs need both. Store lengths as dimensioned quantities, with source expressions where used. Type changes must migrate common values, reset incompatible fields with clear feedback, and regenerate geometry. This migration behavior is proposed; no type change is executed.

### Shaft tab

O behavior — E07. Initially the segment table contains no rows and displays Keine Daten. Pressing the plus adds segment 1, showing Höhe, Oberer Durchmesser, and Unterer Durchmesser. Each value is 10 mm; the lower-diameter field includes fx. A gray wider extension appears above the tool in the live preview. Selecting the row produces a highlighted horizontal band in the preview. A delete control is visible but unused.

P — Represent the shaft as an ordered list of axial segments. For an axisymmetric segment, height and end radii define a cylinder or truncated cone. Assemble segments along the tool axis, with a documented ordering convention. Keep cutter geometry, additional shaft geometry and holder geometry as separate components. Test that segment selection highlights the corresponding component rather than the whole tool.

U — Insertion order for subsequent segments, copying an existing segment, automatic continuity between segments and behavior after deletion are not shown. The default 10 mm segment is observed for this example, not proven universal across all types.

### Holder tab

O control — E08. The left holder list has a search input and Keine Daten empty state. The Info area contains Beschreibung, Produkt-ID, Produkt-Link, Hersteller, and Messlänge. Gauge length shows 0 mm while no holder is selected. Buttons are Halter auswählen, Halter extrahieren, and Halter entfernen. Extraction/removal appear unavailable in this empty state; selection is displayed in a light-blue style. No holder is assigned.

I — The controls imply attaching a reusable holder, extracting an attached holder into a reusable record, and removing the assignment. The extraction destination, reference-versus-copy semantics and editability are U. P — a holder chooser should list permitted holder records; selecting one should update the assembly preview and gauge metadata. Removal should leave cutter and shaft data intact. Extraction should create a holder record with a new identity only after choosing a writable destination.

The generic dimensional illustration on the Tool tab includes a holder-shaped object even while the actual Holder tab is empty. Do not interpret that illustration as evidence that a real holder was assigned to this tool.

### Cutting Data tab

O behavior — E09–E13. The left pane lists Vorgabe-Voreinstellung and later Alu. Adding the new preset opens an inline name field, then produces a separate selectable row. Initial Alu values match the default values. After edits, Alu differs and both presets remain in the saved lower table. I — creation initializes a new independent preset from the selected one; permanent parent-child inheritance is not established.

| Visible field or control | Displayed unit or state | Observed example |
| --- | --- | --- |
| Strategiezuordnung bearbeiten | Button | Strategy-assignment editor available but unopened |
| Spindeldrehzahl | U/min, rpm | 5000 in default; 24000 in Alu |
| Schnittgeschwindigkeit | m/min; fx | 94.24778 then 452.38934 |
| Rampen-Drehzahl | U/min; fx | Follows spindle speed in example |
| Schnittvorschub | mm/min | 1000 then 2000 |
| Vorschub pro Zahn | mm; fx | 0.06667 default; 0.02778 final Alu |
| Einfahrvorschub | mm/min; fx | Lead-in 1000 then 2000 |
| Ausfahrvorschub | mm/min; fx | Lead-out 1000 then 2000 |
| Übergangsvorschub | mm/min; fx | Transition 1000 then 2000 |
| Helixvorschub | mm/min | 333.33 in demonstrated preset |
| Rampenwinkel | Grad, degrees | 2 degrees |
| Eintauchvorschub | mm/min | 333.33 |
| Eintauchvorschub pro Umdrehung | mm; fx | 0.06667 default; 0.01389 final Alu |
| Tiefenzustellung verwenden | Unchecked checkbox | Use stepdown control; subordinate fields not exposed |
| Querzustellung verwenden | Unchecked checkbox | Use stepover control; subordinate fields not exposed |
| Kühlmittel | Dropdown | External water initially; mist final Alu |

The controls are grouped into strategy assignment, speed, feedrates, vertical feedrates, passes/linking, and coolant. P — these groups should be schema-driven so different tool categories can expose the appropriate parameters. Do not assume waterjet, probes and turning tools share this exact field set.

O control — E13. Coolant offers Deaktiviert, Außenkühlung Wasser, Nebel (MMS), Innenkühlung Wasser (IKZ), Außenkühlung Luft, Innenkühlung Luft, Absaugung, Wasser und Nebel (MMS), and Wasser + Innenkühlung (IKZ). Working equivalents are disabled, external water, mist/minimum-quantity lubrication, through-tool water, external air, through-tool air, suction, water plus mist, and external plus through-tool water. O behavior — Nebel (MMS) is selected and retained in the saved Alu row.

P — Use stable coolant enums and map them to a machine/postprocessor adapter. Combined options need explicit representation. A displayed choice does not establish that every machine supports it. U — strategy mapping content, subordinate stepdown/stepover editors and machine-specific coolant output.

### Postprocessor tab

| Visible field | Observed value | Proposed meaning and type |
| --- | --- | --- |
| Nummer | 1 | Tool number; integer |
| Längenversatz | 1, fx and spinner | Length offset register/index; expression-capable integer |
| Durchmesserversatz | 1, fx and spinner | Diameter compensation register/index; expression-capable integer |
| Revolver | 0, spinner | Turret setting; integer |
| Kommentar | Empty | Tool comment; optional string |
| Manueller Werkzeugwechsel | Unchecked | Manual tool change flag |
| Angetriebenes Werkzeug | Checked | Live/rotating tool flag |
| Bruchkontrolle | Unchecked | Breakage-check request flag |

O control — E14. The tab is visited but no post fields are edited. D — Official terminology confirms the offset values are NC identifiers rather than physical offsets in millimeters, and break control depends on machine support. P — validate the integers against the selected machine/post capability. Serialize the metadata into a postprocessor contract; do not emit G-code by hardcoded universal rules. The recording contains no generated NC output or actual breakage sensing.

## 6 Preview help and feedback behavior

O control/behavior — E02, E06 and E07. Both the browser Info pane and editor display a front-view grid with +Z upward, +X to the right, a black reference point at the tool tip, a VORNE orientation indicator, and a scale bar. Scale annotations of 1 mm and 5 mm appear as view framing changes. Geometry edits and shaft creation update the visible shape.

I — Preview framing adapts to geometry or available pane size. Manual orbit, pan and zoom behavior are not demonstrated. P — give the renderer a geometry snapshot and selected parameter/segment ID. Fit the assembly to available space while preserving the tip origin. Distinguish cutting, shaft and holder meshes. Rendering and model validation must share one geometry builder so a preview cannot silently differ from the saved tool.

O behavior — E04 and E13. Hovering fields shows contextual help. Vendor help exposes parameter name, value and expression; plunge-feed help contains an explanatory image and text about axial movement. These are two distinct tooltip styles: parameter diagnostics and operational guidance. P — define reusable tooltip content in the parameter schema; include localized descriptions, expected units and optional diagrams. Show internal parameter identifiers in an advanced view when useful.

O behavior — E11. Error messages appear beside affected fields, red error symbols mark tabs and the preset row turns red/italic. A Problems tab appears and OK becomes unavailable. Its contents are not opened, so an aggregated error-list design is P. E15 shows a green success notification reading Werkzeug erfolgreich erstellt. The notification has a dismiss control and later disappears. E15 also shows a close tooltip naming Escape as a shortcut; pressing Escape itself is not recorded.

P — Provide disabled-state reasons, visible keyboard focus, accessible labels for icon buttons, text equivalents for colors and a live announcement for commit results. Escape should close the appropriate current layer; a dirty draft should have an explicit discard policy. Tooltips should not prevent reaching controls at the bottom of the form.

## 7 Demonstrated workflows and proposed alternate paths

### Workflow A Inspect an existing tool

O behavior — E01–E02. The library opens with no selected tool. Selecting the existing tapered mill populates Info and its cutting-data row. The preview shows a tapered shape. P — selecting a different tool should replace all detail panes consistently; selecting a preset should not change the tool selection. Filtering away the selected tool should clear its selection or explicitly retain it outside the result set.

### Workflow B Create and commit a flat end mill

1. O behavior — E03: invoke New Tool with the plus and choose Schaftfräser.
2. O behavior — E04: enter description, supplier and product ID.
3. O behavior — E05–E06: set flute count and geometry; inspect type/material options and dimension help.
4. O behavior — E07: visit Shaft and add a 10 mm high segment with 10 mm end diameters.
5. O control — E08: visit Holder; the list is empty and no holder is assigned.
6. O behavior — E09: add a preset named Alu while retaining the default preset.
7. O behavior — E10–E12: change spindle speed, encounter expression errors, correct the input, and change cutting feed.
8. O behavior — E13: select mist coolant and inspect plunge-feed help.
9. O control — E14: inspect post settings; leave the visible values as shown.
10. O behavior — E15: confirm with OK; the new document tool and two preset rows appear; a success notification is shown.
11. O behavior — E16: close the library and return to the Manufacture workspace.

There is no demonstrated assignment of the new tool to an operation, regeneration of a toolpath, simulation, or NC export. Existing operation nodes remain visible after closing the library. Do not present the toy model as evidence that the new tool machined it.

### Workflow C Recover from an invalid parameter

O behavior — E10–E12. While editing spindle speed, the UI offers parameter autocomplete. An expression containing an unrecognized U reference and an appended parameter token produces an error. Spindle-dependent fields fail to resolve tool_spindleSpeed. The UI also flags cutting-feed and plunge-feed relationships. Correcting the spindle input to a valid 24000 U/min restores derived values and removes the error markers, with OK enabled again.

P — preserve the user's raw invalid input so it can be corrected; never replace it with a silently clamped value. Propagate invalid dependency status to the affected fields, but explain the root cause first. Continue allowing navigation to other tabs. Reevaluate the aggregate after each confirmed input change, clearing stale messages once their causes disappear.

### Alternate workflows requiring implementation decisions

P — Edit an existing tool by opening a draft from its current revision; duplicate a tool with a new identity; copy it into a different writable library; delete it after resolving operation references; cancel a dirty editor; import/export portable libraries; assign/remove/extract a holder; reorder and copy presets; filter/search for a tool; use a preset in a machining operation. Most of these have visible affordances or source-tree context, but their detailed interactions are not recorded.

For each alternate path, define write permissions, identity semantics, error recovery and effect on document operations. A library deletion must not strand machining operations. Copying a tool should either retain an explicit provenance link or become an independent record; both are defensible designs, but the policy must be visible to users.

## 8 Calculation and expression requirements

### Observed dependencies and numerical fixtures

O behavior — E09–E12. The values are consistent with the following relationships for the demonstrated mill. Here D is diameter in millimeters, n spindle revolutions per minute, z flute count, F cutting feed in mm/min, and Fp plunge feed in mm/min. The equations are derived from the displayed numbers and corroborated for feed per tooth by official parameter documentation; the exact stored expression text is not shown for every field.

```text
surface_speed_m_per_min = pi * D * n / 1000
feed_per_tooth_mm = F / (n * z)
plunge_feed_per_revolution_mm = Fp / n
```

| State | Inputs | Displayed or expected calculated result |
| --- | --- | --- |
| Default preset | D=6, z=3, n=5000, F=1000 | Surface speed 94.24778 m/min; feed/tooth 0.06667 mm |
| Spindle changed | D=6, z=3, n=24000, F=1000 | Surface speed 452.38934 m/min; feed/tooth 0.01389 mm |
| Final Alu | D=6, z=3, n=24000, F=2000 | Surface speed 452.38934 m/min; feed/tooth 0.02778 mm |
| Vertical feed | n=24000, Fp=333.33 | Feed/revolution about 0.01389 mm |

O behavior — Ramp spindle follows spindle speed in this example, and lead-in/out/transition feeds follow the changed cutting feed. Helix and plunge feeds remain 333.33 after cutting feed reaches 2000. Do not assume every motion feed is permanently linked or proportional to cutting feed.

The displayed default plunge feed is 333.33 with feed/revolution 0.06667. This suggests higher internal precision or display rounding; U — exact internal precision. P — keep full precision internally and round only for display. Numerical tests should use absolute/relative tolerance and must not demand bit-for-bit agreement with rounded labels.

### Proposed parameter engine

P — A parameter consists of a stable ID, dimension, permitted value range, raw literal or expression, evaluated quantity, provenance, and validation status. Store the expression and evaluated value separately. fx is an affordance for expression-capable parameters; its presence alone does not prove a field is currently formula-driven.

P — Parse arithmetic and a constrained set of parameter references into an abstract syntax tree. Use a unit registry and a dependency graph. Evaluate in topological order; detect cycles, unknown identifiers, invalid units, zero divisors, nonfinite results and invalid domain values. Never evaluate expressions as unrestricted JavaScript or executable code. Enforce bounded expression length and calculation cost.

P — Autocomplete should offer identifiers valid in the current tool/preset context and should insert text with a defined replacement range. An expression-edit view should show raw expression, result and units without contaminating the value with an appended suggestion. The malformed U reference seen in the demo is a useful regression scenario; reproducing the input mistake is not a feature requirement.

P — Avoid automatic circular equations. Choose driving values explicitly: spindle or surface speed; cutting feed or chip load; plunge feed or feed/revolution. When a dependent value is overridden, replace its driver or switch calculation mode through a visible rule. Do not silently maintain two incompatible drivers.

O behavior — The UI enforces consistency between cutting feed, flute count, feed/tooth and spindle speed, and between plunge feed, feed/revolution and spindle speed during the invalid state. P — display a root diagnostic plus dependent-impact messages. U — whether all these consistency checks are always blocking or only emitted when expressions fail.

### Validation rules for the new implementation

P — For rotating mills, require finite positive diameter, positive rpm and a positive integer flute count before resolving chip load. Validate positive segment height and diameters; type schemas may allow zero at a true pointed tip. Validate flute length no greater than shoulder length, and shoulder length no greater than overall length for the flat mill schema. The demonstrated 20, 21 and 56 mm values satisfy this proposed relationship. Do not apply this rule unchanged to every process type.

P — Distinguish hard errors from warnings. Unknown parameters, incompatible units and impossible core geometry block commit. Catalog metadata omissions can be optional. Machine limits, unsupported coolant or missing holder information become context-specific warnings/errors only when an operation or postprocessor is selected. A tool catalog record should not require a machine merely to be saved.

P — String and URL validation must be separate from geometry validation. Product IDs must preserve string semantics. Empty comments and links are valid. Name uniqueness should be determined per library and must not be confused with NC tool-number uniqueness in a setup or output program.

## 9 Functional requirements and acceptance basis

These requirements describe the new software. The Basis column ties them to observed controls/behavior or identifies proposal-only scope. Acceptance criteria strengthen observed behavior into a testable contract without claiming that every edge case is present in Fusion.

| ID | Requirement | Basis |
| --- | --- | --- |
| FR01 | Browse hierarchical sources and grouped tool records | O control E01 |
| FR02 | Coordinate tool selection, preview, Info and preset table | O behavior E02/E15 |
| FR03 | Provide category chooser with all 33 creation tiles and a schema for the additional visible form-mill type | O control E03/E05; creation path for form mill U |
| FR04 | Create a flat mill with all six editor tabs | O behavior/control E04–E14 |
| FR05 | Store vendor/catalog metadata independently of numerical geometry | O behavior E04/E06; user clarification |
| FR06 | Edit dimensions and update preview and dimension help | O behavior E06 |
| FR07 | Add ordered shaft segments and highlight selection | O behavior E07; ordering semantics P |
| FR08 | Support optional holder search, assignment, extraction and removal | O control E08; semantics P |
| FR09 | Maintain multiple named presets with independent values | O behavior E09/E15 |
| FR10 | Recalculate parameter dependencies and retain raw expressions | O behavior E10–E12; persistence format P |
| FR11 | Show root/dependent validation errors and block invalid commit | O behavior E11 |
| FR12 | Offer all nine visible coolant choices | O control E13; mist selection O behavior |
| FR13 | Persist all visible postprocessor metadata | O control E14; NC export P |
| FR14 | Commit the complete tool and show a success result | O behavior E15; atomicity P |
| FR15 | Supply contextual help and keyboard/accessibility support | Tooltips O behavior; most accessibility P |
| FR16 | Search, filter and configurable/sortable table properties | O control E01; detailed semantics P/D |
| FR17 | Copy, edit, delete, renumber and cleanup commands with explicit semantics | Mostly icon-based I; specific command definitions U |
| FR18 | Integrate saved tools/presets with document operations | Existing context O control E16; new-tool assignment P |
| FR19 | Persist, version, migrate and exchange portable records | P; restart/import/export not shown |
| FR20 | Provide cloud/catalog adapters and permission-aware edits | Source labels O control E01; service behavior P |

FR01–FR16 are the core visible UI scope. FR17 needs command verification before being advertised as parity. FR18–FR20 extend the design into a usable software product; the recording cannot prove their complete behavior.

## 10 Proposed architecture

### Component boundaries

P — Use a local-first modular architecture. The same domain package should power editor validation, persistence validation, previews and operation adapters. The UI must not own independent copies of calculation rules. Framework and database selection remain implementation decisions; the recording reveals no actual frontend, backend, database or service design.

```text
Library browser and tool editor
             |
       Application commands
             |
   Tool domain and draft transactions
      /          |            \
Type schemas  Parameter engine  Geometry builder
      \          |            /
       Aggregate validation and snapshots
             |
     Repository and library providers
        /        |         \
  Document     Local     Cloud or catalog
             |
     CAM operation and post adapters
```

P — Browser/editor components manage selection, focus, navigation and presentation. Application commands open drafts, mutate them, validate, commit, duplicate and remove records. The domain model owns invariants and typed geometry. The parameter engine resolves expression dependencies. The geometry builder produces a common geometric snapshot for rendering and downstream consumers.

P — Repositories implement local and document persistence through transactional writes. Library providers expose tree nodes, capabilities and records for built-in and supplier catalogs as well as writable sources. Optional cloud adapters implement synchronization and shared access. CAM adapters resolve a tool/preset revision for an operation; post adapters map tool number, compensation identifiers, coolant and flags to supported NC semantics.

### Suggested persistence and transaction flow

P — Store authoritative local records in a transactional store, with schema version and revisions. A file-backed store is sufficient for a single-user application; an embedded relational store simplifies querying and atomic multi-entity writes. Export a versioned portable JSON representation for interchange. Do not copy undocumented Fusion file formats solely from the appearance of its UI.

P — Begin an editor with a deep draft of a known revision. Apply field changes to the draft, recompute affected parameters, produce diagnostics and update preview. Commit only if aggregate validation succeeds. Save tool, geometry, presets, assembly assignment and post settings in one transaction. Return the committed revision and select it in the browser. Display the success notification only after persistence completes.

P — Use optimistic concurrency for edits and cloud synchronization. If the base revision changed, preserve the draft and offer comparison or resolution. Keep catalog records immutable locally unless copied into a writable library. A cloud outage must not erase saved local tools or block editing a local/document library.

### Preview and responsiveness

P — Generate simple axisymmetric mill/shaft geometry from profiles. Use separate generators for non-axisymmetric turning/probe/process types. Run expensive geometry rebuilding or validation off the UI thread. Attach a monotonically increasing draft sequence number to requests, discarding stale preview/validation results that arrive after a newer edit.

P — Cache previews by geometry and assembly revision, not description alone. Virtualize long tool tables and preserve row identity during sorting/filtering. Proposed acceptance targets are under 100 ms for ordinary field feedback and under 300 ms for simple preview updates on agreed reference hardware. These are product targets, not measured recording performance.

### Integration boundaries and machine support

P — Operation records should reference a resolved tool revision, preset revision and any operation overrides. Keep library settings distinguishable from operation-specific feed overrides. Changing a library preset must not silently rewrite an approved operation; use an explicit update/relink policy. Mark affected toolpaths stale if their resolved geometry or cutting-data inputs change.

P — A machine capability record declares allowed tool numbers, offset ranges, tool-change behavior, coolant support, spindle limits and break-control support. Postprocessing consumes this record. Neither a coolant selection nor a live-tool checkbox proves machine compatibility. Simulation and collision detection can consume the assembly geometry later; they are not observed workflows.

## 11 Proposed data model

### Entity relationships

```text
LibraryNode 1 -> many LibraryNode children
Library 1 -> many ToolRecord
ToolRecord 1 -> 1 typed CutterDefinition
ToolRecord 1 -> many ShaftSegment
ToolRecord 1 -> zero or 1 HolderAssignment -> HolderRecord
ToolRecord 1 -> many CuttingPreset
ToolRecord 1 -> 1 PostSettings
CuttingPreset 1 -> many ParameterValue
CuttingPreset many -> many StrategyReference through StrategyMapping
Document 1 -> many Setup 1 -> many OperationToolBinding
OperationToolBinding -> resolved ToolRecord and CuttingPreset revisions
```

P — A hierarchy node and a storage library are different entities. Category headings or a document/setup grouping can be navigation nodes without owning duplicate tool definitions. Source scope and catalog permissions are metadata, not geometry properties.

| Entity | Key fields | Constraints and rationale |
| --- | --- | --- |
| LibraryNode | ID, parent ID, label, node kind, provider ID, source ref | Acyclic tree; labels localized or user supplied |
| Library | ID, node ID, source kind, enabled state, permissions, revision | Separate availability, enabled state and read/write capability |
| ToolRecord | ID, library ID, type ID, category, metadata, revision, source provenance | Stable identity independent of generated name and tool number |
| ToolMetadata | Description, vendor, product ID, product URL | Strings; product ID retains leading zeros |
| CutterDefinition | Discriminator, units, rotation, flute count, material, typed geometry | Schema validation for the selected type |
| ShaftSegment | ID, tool ID, order, height, top/bottom diameter | Ordered positive geometry with parameter expressions where allowed |
| HolderRecord | ID, library ID, metadata, gauge length, geometry | Reusable independent record |
| HolderAssignment | Tool ID, holder ID or embedded revision, placement | Optional; explicit reference/copy policy |
| CuttingPreset | ID, tool ID, name, order, parameter set, coolant, flags, revision | At least one valid preset in proposed mill MVP |
| ParameterValue | ID, dimension, raw expression, resolved quantity, driver mode | Preserve intent, precision and dependency diagnostics |
| StrategyMapping | Preset ID, strategy ID, optional context | Mapping schema proposed; dialog content U |
| PostSettings | Tool number, offset indices, turret, comment, booleans | Integer index semantics; machine-aware validation |
| OperationToolBinding | Document/setup/operation IDs, resolved tool/preset revision, overrides | Prevent silent mutation of operation inputs |
| Revision | Entity ID, version, base revision, timestamps, change summary | Migration and concurrent-edit support |
| ValidationIssue | Path, code, severity, message, root path, blocking flag | Draft/runtime state; avoid persisting stale derived errors |

### Units expressions and serialization

P — Store dimensions in a canonical unit system, while retaining preferred display units on the tool and original expressions. A serialized quantity should include value and dimension rather than an ambiguous number. A literal 6 mm, an expression linked to cutter diameter, and a rounded display of 6 must remain distinguishable.

P — Use stable type IDs such as milling.flat_end_mill rather than translated labels. Use stable parameter paths such as cutter.geometry.diameter and presets/{presetId}/spindleSpeed. Provide an alias registry when supporting recognizable diagnostic identifiers such as tool_spindleSpeed. Do not assume an undocumented Fusion variable namespace is a mandatory compatibility contract.

P — Serialize schemaVersion, record identity, revision, source provenance, all presets, and post settings. Validate imported records before installation. Preserve supported unknown extension fields without executing them. Reject or quarantine incompatible geometry/expression records with diagnostics rather than silently dropping fields.

### Demonstrated record as a fixture

| Property | Recorded final value | Treatment |
| --- | --- | --- |
| Destination | Documents / Tigerente v7 | O behavior E15 |
| Type and units | Flat end mill; millimeters | O control/behavior E05/E15 |
| Supplier and product ID | Aliexpress; 567954 | O behavior E04 |
| Description | Schaftfräser 5mm | User-confirmed demo error; corrected fixture description should say 6 mm |
| Rotation and flute count | Clockwise; 3 | O control/behavior E05 |
| Material | HSS | O control E05 |
| Core geometry | Diameter 6, shaft diameter 6, overall 56, below holder 24, shoulder 21, flute 20 mm | O behavior/control E06/E15 |
| Assembly gauge | 24 mm | O control E06/E15 |
| Extra shaft | One segment: height 10, upper/lower diameter 10 mm | O behavior E07 |
| Holder | No assignment demonstrated | O control E08; inferred absence from empty state |
| Default preset | 5000 rpm; 1000 mm/min; external water | O control E09/E15 |
| Alu preset | 24000 rpm; 2000 mm/min; mist | O behavior E12/E13/E15 |
| Post fields | Number/offsets 1; turret 0; live tool true; manual change/break control false | O control E14 |

The name Alu is a free-form preset label. It does not establish that a structured aluminum material record was selected, or that the settings are validated for aluminum.

## 12 Proposed command and service contracts

P — These logical contracts can be implemented in-process for a desktop app or behind HTTP/IPC boundaries. No API transport is observed. Every mutating command returns structured diagnostics and an authoritative revision; reads return permission/capability metadata alongside records.

| Contract | Essential input | Result and failure behavior |
| --- | --- | --- |
| listLibraryNodes | Parent/source scope | Children with availability and permissions |
| queryTools | Scope, query, category, sort, cursor | Stable paged rows; empty result is valid |
| getToolAggregate | Tool ID and optional revision | Geometry, assembly, presets, post data and provenance |
| beginToolDraft | Library ID, type ID, optional source tool/revision | Draft ID, initialized fields and capabilities |
| updateDraftParameter | Draft ID, sequence, parameter path, raw value | Resolved values, issues and preview version |
| addOrRemoveSegment | Draft ID, segment ID/order, parameters | Updated ordered assembly and diagnostics |
| createPreset | Draft ID, name, optional source preset ID | Independent preset with new ID |
| assignHolder | Draft ID, holder ID/revision, copy policy | Validated assignment and new preview snapshot |
| validateDraft | Draft ID, optional machine context | Aggregate issues, commit eligibility |
| commitDraft | Draft ID, base revision, idempotency token | Persisted aggregate or conflict/validation/storage error |
| discardDraft | Draft ID | Removes uncommitted draft; original record unchanged |
| copyTool | Source ID/revision, target library, naming policy | New independent identity with provenance |
| deleteTool | Tool ID, expected revision, reference policy | Removed/tombstoned record or list of blocking references |
| resolveOperationTool | Operation context, tool/preset revisions | Immutable machining input snapshot and capability issues |
| importOrExportLibrary | Versioned payload, destination/selection | Migration report; atomic import where possible |

P — Use errors such as UNKNOWN_PARAMETER, UNIT_MISMATCH, DEPENDENCY_CYCLE, INVALID_GEOMETRY, READ_ONLY_LIBRARY, REVISION_CONFLICT and STORAGE_FAILURE. Error messages should include affected paths and actionable recovery, while keeping provider credentials and internal exception traces out of user-facing notices.

## 13 Implementation backlog

P — Priorities below are recommendations for delivering the recorded flat-mill workflow first while keeping the broader visible catalog in the product plan. P0 is necessary for the demonstrated vertical slice; P1 completes visible affordances and adjacent core workflows; P2 covers integrations whose execution is unshown. A complete visible feature implementation includes all 34 type labels and their agreed schemas, not only the P0 slice.

### B01 P0 Establish schemas identities and library storage

Create the tool aggregate, stable type/parameter IDs, revision handling and a writable local/document repository. Basis: E01, E04–E15. Acceptance: one commit stores metadata, geometry, segments, two presets and post settings together; a simulated write failure stores no partial record; reload yields the same IDs and resolved values.

### B02 P0 Implement browser selection and detail panes

Build the source tree, grouped tool table, cutting-data table and Filter/Info shell. Basis: E01/E02/E15. Acceptance: no selection shows the prompt; selecting a tool fills both dependent panes; switching tools never exposes stale preset/preview data; after commit the new tool is selected in the correct document group.

### B03 P0 Implement the creation chooser and flat mill editor

Expose the 33 recorded chooser labels and establish a declared availability status for each type. Implement General, Tool and Postprocessor data for flat mills. Basis: E03–E06/E14. Acceptance: choosing a flat mill opens a draft; all recorded fields exist; tab navigation retains values; cancel discards the draft. Unimplemented non-MVP types must be clearly unavailable rather than opening misleading generic forms.

### B04 P0 Implement the quantity and expression engine

Support units, constrained expressions, dependency evaluation and raw/evaluated values. Basis: E10–E12. Acceptance: the 6 mm, 3-flute fixtures produce the Section 8 numbers within display tolerance; unknown references and cycles return diagnostics; invalid expressions never execute code; dependent invalid values do not become zero by default.

### B05 P0 Implement aggregate validation and error recovery

Add field issues, tab indicators, preset error state, Problems view and disabled commit. Basis: E11/E12. Acceptance: a malformed spindle expression disables OK and marks the dependent feed/speed fields; entering 24000 rpm clears relevant issues without deleting Alu; fixing one root cause removes its stale dependent errors.

### B06 P0 Build geometry previews and shaft segments

Provide the front preview, origin, scale, material regions and dimension/segment highlighting. Basis: E02/E06/E07. Acceptance: adding a 10 by 10 by 10 mm segment produces a wider axial section; editing a dimension updates the snapshot; selected segment highlighting follows its ID; preview and exported geometry share the same dimensions.

### B07 P0 Implement independent cutting presets and coolant

Add the default preset and inline creation of Alu, parameter groups and all nine coolant enums. Basis: E09–E13/E15. Acceptance: Alu can be changed to 24000 rpm and 2000 mm/min with mist while the default remains 5000 rpm and 1000 mm/min; both rows survive commit/reload; generated dependent values respond correctly.

### B08 P0 Complete transactional commit and result feedback

Wire OK, Cancel, commit eligibility, selection refresh and success/failure notices. Basis: E11/E15. Acceptance: success is announced only after saving; repeated submissions with the same token do not duplicate the tool; failure retains the editor draft; clean close returns to the host workspace.

### B09 P1 Implement search filters and table configuration

Define search scope, category filters, Clear Filters, sort and column behavior. Basis: E01, with exact semantics P/D. Acceptance: query plus category returns only matching records; sort is stable for equal diameters; no-results state is distinguishable from no selection; resizing or hiding columns does not lose selection.

### B10 P1 Implement holder workflows

Support holder records, search, assignment, extraction and removal. Basis: E08. Acceptance: empty lists show the recorded empty state; assigning a holder updates assembly geometry; removal preserves cutter and shaft; extraction requires a writable destination and creates a new holder identity. Confirm the intended gauge-length reference before completion.

### B11 P1 Complete every milling type schema

Implement the other 14 chooser milling types plus the form-mill type exposed in the dropdown. Basis: E03/E05. Acceptance: each has an agreed parameter schema, type validation, distinct preview and explicit operation compatibility; switching type never retains hidden incompatible dimensions; form profiles are versioned and validated. The detailed form-import/editor path requires a product decision.

### B12 P1 Complete drilling and turning schemas

Implement the nine hole-making and four turning labels. Basis: E03. Acceptance: left/right tap direction is explicit; pointed-tip zero diameter is handled by type rules; turning insert orientation and envelope are represented without treating every cutter as an axisymmetric mill. Verify unshown parameter requirements with domain review.

### B13 P1 Complete cutting probing and holder creation

Implement waterjet, laser, plasma, probe and holder definitions. Basis: E03. Acceptance: none requires irrelevant mill flute-count inputs; each exposes agreed category-specific settings and preview; unsupported machine/process settings produce explicit capability messages. The footage proves type availability only, so schemas require independent design validation.

### B14 P1 Implement preset management and strategy mapping

Add rename, sort, delete, copy/paste and explicit strategy associations. Basis: visible controls E09 plus D preset documentation. Acceptance: copied presets have new IDs; formulas resolve against the target tool; incompatible types are rejected; sorting is deterministic; deleting a preset referenced by operations presents the selected reference policy. Strategy selection UI requires design confirmation.

### B15 P1 Resolve and implement library toolbar commands

Confirm the meanings of edit, duplicate/copy/paste, print, numbering and cleanup icons before freezing requirements. Basis: icon inference from E01/E02. Acceptance: every command has an accessible name, capability predicate, target scope and documented side effect; multi-selection policy and reference protection are tested. Cleanup must not remove used tools without a defined policy.

### B16 P1 Add context help localization and accessibility

Implement parameter diagnostics and illustrated operational tooltips, keyboard navigation and translated labels. Basis: E04/E13/E15. Acceptance: all icon-only actions have names; focus remains visible; errors are readable without color; Escape operates on the current dialog layer; a translated UI preserves stable type and parameter IDs.

### B17 P2 Add operation binding and machine aware posting

Connect tool/preset revisions to document setups and operations and supply a postprocessor contract. Basis: E16 context and E14 controls. Acceptance: operation overrides remain separate; changing a source tool offers an explicit update; tool/offset indices are validated against machine support; unsupported coolant and break-control requests cannot silently emit incorrect NC semantics.

### B18 P2 Add catalogs cloud sync and portable exchange

Create provider adapters, permissions, import/export and optional shared revisions. Basis: E01 source labels; implementation P. Acceptance: read-only catalogs require copying before edit; local work continues offline; revision conflicts preserve both drafts; imported invalid expressions are quarantined; a versioned round trip preserves all supported tool/preset fields.

## 14 Verification plan and unresolved decisions

### Tests with direct recording evidence

P — Automate the create-tool journey and validate the saved aggregate, not merely the presence of controls. Check empty-to-selected browser states, every demonstrated editor tab, creation of a shaft segment, independent default/Alu presets, all coolant labels, the invalid-expression cycle, and successful commit. Compare preview dimensions numerically; screenshot checks may complement geometry tests but must not be the only validation.

P — Use the corrected fixture description “Schaftfräser 6mm” with D=6. A separate metadata test may show that a description does not numerically drive geometry, but the known demonstration error must not be treated as the desired default record. Validate product ID as a string and preserve 567954 on reload.

P — Test that n=0, z=0 and an unknown spindle reference produce useful calculation errors rather than infinities. Change F from 1000 to 2000 at n=24000 and ensure linked lead-in/out/transition values follow only if their configured expression links remain intact. Confirm helix/plunge values do not change simply because cutting feed changed.

### Additional engineering tests

P — Test unit conversion round trips, expression cycles and dimension mismatches; type-change migration; segment deletion/reordering; holder assignment/removal; write failures and optimistic conflicts; operation-reference protection; catalog permissions; import validation; stale asynchronous preview results; accessible keyboard completion; and long/truncated tool names. Measure performance with a representative large library rather than estimating it from video timing.

### Unknowns requiring resolution

| Decision or missing evidence | Why it matters | Recommended action |
| --- | --- | --- |
| Search scope and combined filter rules | Prevent inconsistent navigation | Decide and document scope before UI implementation |
| Toolbar icon semantics | Avoid inventing commands from graphics | Inspect tooltips/context menus or validate with product owner |
| Form mill creation path and profile format | Label exists only in type menu | Design or verify a supported profile workflow |
| Unvisited type-specific fields | Complete catalog needs more than labels | Obtain additional recordings or approve new schemas |
| Holder extraction and gauge reference | Controls visible; action unshown | Specify copy/reference and coordinate conventions |
| Strategy-assignment dialog contents | Mapping button visible only | Define strategy IDs, rules and selection UI |
| Stepdown/stepover enabled controls | Checkboxes unselected | Define values, units and override behavior |
| Library versus operation update policy | Existing setups visible; new assignment absent | Choose explicit revision binding policy |
| Persistence across restart and cloud sync | Save visible only within session | Specify storage durability and offline/conflict behavior |
| Import/export file formats | No file dialogs shown | Choose versioned portable format; do not claim Fusion compatibility |
| Problems tab interaction | Tab appears but is unopened | Implement aggregated, navigable diagnostics as proposal |
| Manual preview navigation | Grid/scale visible; orbit not exercised | Define interaction separately from auto-fit |
| NC program, simulation and machine behavior | No generated output or execution shown | Treat as integration scope with independent tests |

## 15 Supplementary official references

These sources were checked on 14 October 2026. They are D evidence and do not convert undocumented footage behavior into observed facts. The recording, rather than current documentation, determines the visible UI inventory in this reference.

[Autodesk Tool Library overview](https://help.autodesk.com/view/fusion360/ENU/?contextId=MFG-REF-TOOL-LIBRARY-NEW) describes the three library sections and supports configurable table properties and multiple cutting-data sets. These table capabilities are documented; only the visible columns and sort indicator are observed here.

[Autodesk Types of tool libraries](https://help.autodesk.com/view/fusion360/ENU/?contextId=MFG-TOOL-LIB-TYPES) documents document/local/cloud sources, read-only built-in/vendor catalogs, and copying selected catalog tools into a document. Those permission/copy semantics are D, not demonstrated source mutations. The source also notes varying availability of hub/vendor libraries; their labels are visible in this recording.

[Autodesk Work with tool presets](https://help.autodesk.com/cloudhelp/ENU/Fusion-CAM/files/MFG-CREATE-TOOL-PRESET.htm) documents initialization from a selected/default preset, rename/sort/delete, copy/paste to tools of the same type, and drag reordering. The video demonstrates preset creation and independent edited values, but not drag reordering or cross-tool copying.

[Autodesk Milling or Hole Making tool reference](https://help.autodesk.com/cloudhelp/ENU/Fusion-CAM/files/GUID-A0197982-0082-45AF-B848-8D62CAF57C49.htm) clarifies shaft segment dimensions and optional holders. It identifies postprocessor offsets as NC register identifiers and describes machine-dependent break control. These definitions support interpretation of visible fields, not a claim of NC generation in the recording.

[Autodesk CAM expression variables](https://www.autodesk.com/support/technical/article/caas/sfdcarticles/sfdcarticles/CAM-Expression-Variables-in-Fusion-360.html) includes feed-per-tooth and ramp-spindle relationships using tool parameter identifiers. It corroborates the displayed dependency behavior. The new implementation should publish its own supported variable contract rather than promise full compatibility with an undocumented parser.

## 16 Visual evidence plate index

The Word document includes six embedded native-resolution evidence frames. This text edition retains their captions and timestamps below. Refer to the evidence ledger for other intervals.

Figure 1 E01 at 00:00 — Three-region library browser with no tool selected. The source tree, tool columns, category filters and lower empty state are visible.

Figure 2 E03 at 00:12 — All 33 creation tiles are visible. The highlighted ball-end tile is a hover state; the demonstrated editor is the flat end mill selected afterward.

Figure 3 E06 at 01:03 — Flat mill parameters and dimension help. The “5mm” description is the user-confirmed demonstration error; the numeric diameter is 6 mm.

Figure 4 E07 at 01:09 — A shaft segment with height and end diameters of 10 mm; selection highlights the corresponding axial region.

Figure 5 E11 at 01:27 — The invalid spindle expression causes inline/dependent diagnostics, red tab indicators, a Problems tab and disabled OK.

Figure 6 E15 at 02:00 — Saved document tool and two cutting presets with different feeds, speeds and coolant, plus a successful creation notification.
