# ObjectLift 0.4.1

- Locks the visible selection on mouse-down so a newly discovered nested part or stale model result cannot replace it.
- Saves transparent PNG, lossless WebP, and source and mask ZIP on a background worker.
- Includes experimental editable PSD export with a real pixel mask and hidden original crop.
- Adds the ObjectLift icon to Explorer, the overlay window, and the tray.

Download and extract the Windows x64 ZIP. Run Setup-ObjectLift.cmd once, then Start-ObjectLift.cmd. Setup needs an internet connection. Capture, inference, and export run locally afterward.

The build is unsigned. The baseline model setup supports Fast mode. Optional advanced inference requires separately prepared assets. PSD structure was checked with an independent reader. Photoshop edit, save, and reopen verification remains open.
