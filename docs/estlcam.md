# Estlcam CNC export

In the Machine inspector, select **Estlcam** as the post. Under **Coolant / MMS
outputs**, enable the output wired to the lubrication system, then click **Apply**:

| Setting | On | Off |
| --- | --- | --- |
| Output 1 (enabled by default) | M8 | M9 |
| Output 2 | M10 | M11 |

Both outputs can be enabled independently. Selected outputs follow the program's
coolant state: flood or mist turns them on; coolant-off turns them off. They are
also switched off at program start and end. Disabled outputs emit no commands.
For an MMS system on output 2, disable output 1 and enable output 2.
Settings are stored with the local machine and restored when selecting it.

Configure Estlcam's program units as **mm** and feed units as **mm/min**. Touch off
the single workpiece origin in Estlcam. The exporter repeats every motion G word,
uses relative I/J arcs, splits full circles and helices into half arcs, and expands
drilling into explicit moves.

Timed dwells, reverse spindle, controller cutter compensation, and multiple work
datums are rejected. Use computed compensation and export origins separately.

Controller requirements: [Estlcam CNC programs](https://www.estlcam.de/steuerung_cnc_programme.php).
