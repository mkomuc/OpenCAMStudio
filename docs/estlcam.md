# Estlcam CNC export

In the Machine inspector, select **Estlcam** as the post. Under **Coolant / MMS
outputs**, enable the output wired to the lubrication system, then click **Apply**:

| Setting | On | Off |
| --- | --- | --- |
| Output 1 (enabled by default) | M8 | M9 |
| Output 2 | M10 | M11 |

Select **Cooling → MMS** in the cutter defaults or cutting preset, or in the
operation's **Tool** tab. **None** keeps cooling disabled. MMS activates the
configured outputs for each cut and turns them off when the operation finishes
and before a tool change. Files without a cooling choice default to None.

Both outputs can be enabled independently. Selected outputs follow the program's
coolant state: flood or mist turns them on; coolant-off turns them off. They are
also switched off at program start and end. Disabled outputs emit no commands.
For an MMS system on output 2, disable output 1 and enable output 2.
Settings are stored with the local machine and restored when selecting it.

Configure Estlcam's program units as **mm** and feed units as **mm/min**. Touch off
the single workpiece origin in Estlcam. The exporter repeats every motion G word,
uses relative I/J arcs, splits full circles and helices into half arcs, and expands
drilling into explicit moves.

Manual tool changes carry the tool details directly on the `M6` line so Estlcam
can display them while waiting for the cutter to be changed, for example:

```gcode
T1 M6 (T1 - 6 mm rougher - Square End Mill - D6.000 - FL18.000 - CR=0.000 - F3 - RPM 12000)
```

The prompt includes the cutter description, type, diameter (`D`), effective flute
length (`FL`), corner radius (`CR`), flute count (`F`), and upcoming operation's
spindle speed (`RPM`). Dimensions are in mm; `F` inside this comment is the flute
count, not a feed command. RPM comes from the toolpath, including operation
overrides. Names are sanitized for G-code comments. Direct post callers can pass
the setup's tools through `PostOptions.tools`; without tool metadata, the prompt
still identifies the tool number and any upcoming RPM.

Timed dwells, reverse spindle, controller cutter compensation, and multiple work
datums are rejected. Use computed compensation and export origins separately.

Controller requirements: [Estlcam CNC programs](https://www.estlcam.de/steuerung_cnc_programme.php).
