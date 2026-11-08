<p align="center"><img src="assets/objectlift.svg" width="104" alt="ObjectLift icon"></p>

# ObjectLift · Auto Image Crop

ObjectLift is a Windows tray utility for cutting out objects from your screen. Freeze the desktop, hover to see the proposed selection, then click to lock it. Refine the cutout and copy it with transparency or save it to a file.

## Install on Windows

1. Download the Windows x64 ZIP from [GitHub Releases](https://github.com/Rajikshank/auto-image-crop/releases/latest) and extract it into a writable folder.
2. Run **Setup-ObjectLift.cmd** once. Setup downloads the pinned MobileSAM model and inference runtime from Hugging Face and NuGet and checks their SHA-256 hashes.
3. Run **Start-ObjectLift.cmd**. Look for the ObjectLift icon in the tray, including the hidden icons area.
4. Press **Ctrl+Shift+O** to snip. Click the highlighted object and use **Copy**, **Inspect**, or **Refine**.

The ZIP is the quickest install. The standalone EXE needs the same `models` folder alongside it. The build is unsigned, so Windows may display a publisher prompt. Verify the download hash before running it. Windows 11 x64 is the tested platform. GPU inference is optional and CPU mode is available from the tray.

## Use ObjectLift

| Goal | Action |
| --- | --- |
| Select the visible highlight | Click the object. ObjectLift locks the mask shown at mouse-down. |
| Try overlapping candidates | Scroll while hovering, or press F6 to browse detected objects. |
| Find a missed object | Draw a loose lasso around it. |
| Clean up edges | Choose Refine and adjust the brush size. Add or remove pixels and use Ctrl+Z/Ctrl+Y for undo/redo. |
| Check transparency | Choose Inspect to cycle backgrounds. |
| Copy | Click Copy or press Enter. The receiving app chooses which clipboard format to use. |
| Save PNG | Press Ctrl+S. |
| Save WebP, source and mask ZIP, or PSD | Select the object, then open More → Export. |
| Zoom | Ctrl+mouse wheel, or + and −. Use 0 to reset the view. |
| Cancel | Esc exits the current action or closes the capture. |

The ZIP export contains the cutout, the original pixels within its crop, a grayscale alpha mask, and position metadata. PSD is experimental and stores an editable pixel mask with a hidden original crop. Independent file readers verified that structure. Photoshop edit, save, and reopen compatibility is still under review.

Some paste destinations flatten alpha. For WhatsApp Web, save the PNG and attach it as a document to transfer the original file.

## Privacy and model setup

Normal capture and inference run locally. The setup script contacts the model and runtime hosts to download dependencies. Capture pixels stay in memory until you copy or save them. A source bundle or PSD includes original pixels from within the crop, so review those before sharing.

The standard setup supports the Fast workflow. The optional Balanced, Quality, and advanced AI refinement features require additional assets built by the source setup script. See [models](docs/MODELS.md) and [build and verification](docs/DEVELOPMENT.md).

ObjectLift source is [MIT licensed](LICENSE). Models and inference runtimes have their own terms. See [third-party notices](THIRD-PARTY-NOTICES.md).
