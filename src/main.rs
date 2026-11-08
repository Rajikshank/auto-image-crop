use anyhow::{Context, Result};
use objectlift::{Frame, extract, inference::Segmenter, platform};
use std::{path::PathBuf, time::Instant};

fn main() {
    if let Err(e) = run() {
        eprintln!("ObjectLift: {e:#}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.as_slice() == ["--version"] {
        println!("ObjectLift {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if args
        .iter()
        .any(|s| matches!(s.as_str(), "--help" | "-h" | "help"))
    {
        println!(
            "ObjectLift — local Windows object snipping\n\nobjectlift [--cpu] [--once]\nobjectlift --demo <image> [--cpu]\nobjectlift --smoke <image> [--cpu]\nobjectlift capture <output.png>\nobjectlift segment <input> <output.png> <x> <y> [--cpu]\nobjectlift probe\n\nDefault shortcut: Ctrl+Shift+O. Right-click the tray icon for settings and Exit."
        );
        return Ok(());
    }
    let models = std::env::var_os("OBJECTLIFT_MODELS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let beside_exe = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.join("models")));
            beside_exe
                .filter(|p| p.is_dir())
                .unwrap_or_else(|| PathBuf::from("models"))
        });
    if args.first().map(String::as_str) == Some("inspect") {
        ort::init_from(models.join("runtime/runtimes/win-x64/native/onnxruntime.dll"))?
            .with_telemetry(false)
            .commit();
        let path = args
            .get(1)
            .context("Usage: objectlift inspect <model.onnx>")?;
        let session = ort::session::Session::builder()?.commit_from_file(path)?;
        println!(
            "inputs={:?}\noutputs={:?}",
            session.inputs(),
            session.outputs()
        );
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("precision") {
        let input = args.get(1).context("precision <input> <output> <x> <y>")?;
        let output = args.get(2).context("output")?;
        let image = image::open(input)?.to_rgba8();
        let frame = Frame::new(image.width(), image.height(), image.into_raw())
            .map_err(anyhow::Error::msg)?;
        let mut engine =
            objectlift::advanced::Sam21::load(&models, !args.iter().any(|s| s == "--cpu"))?;
        let start = Instant::now();
        let result = engine.predict(
            &frame,
            &[(
                args.get(3).context("x")?.parse()?,
                args.get(4).context("y")?.parse()?,
                true,
            )],
        )?;
        let cutout = extract(&frame, &result.mask).map_err(anyhow::Error::msg)?;
        std::fs::write(output, platform::png(&cutout)?)?;
        println!(
            "SAM21 ms={} score={} area={}",
            start.elapsed().as_millis(),
            result.score,
            result.mask.area()
        );
        return Ok(());
    }
    if args
        .first()
        .is_some_and(|s| s == "probe" || s == "paste-probe")
    {
        return objectlift::native_probe::run(
            "artifacts/capture-probe.png",
            args[0] == "paste-probe",
        );
    }
    if args.first().map(String::as_str) == Some("capture") {
        let desktop = platform::capture()?;
        let path = args
            .get(1)
            .context("Usage: objectlift capture <output.png>")?;
        std::fs::write(path, platform::png(&desktop.frame)?)?;
        println!(
            "Captured {}x{} at {},{}",
            desktop.frame.width, desktop.frame.height, desktop.left, desktop.top
        );
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("segment") {
        let input = args
            .get(1)
            .context("Usage: objectlift segment <input> <output> <x> <y> [--cpu]")?;
        let output = args.get(2).context("Output required")?;
        let x: f32 = args.get(3).context("x required")?.parse()?;
        let y: f32 = args.get(4).context("y required")?.parse()?;
        let image = image::open(input)?.to_rgba8();
        let frame = Frame::new(image.width(), image.height(), image.into_raw())
            .map_err(anyhow::Error::msg)?;
        let start = Instant::now();
        let mut engine = Segmenter::load(&models, !args.iter().any(|s| s == "--cpu"))?;
        let loaded = start.elapsed().as_millis();
        let embedding = engine.encode(&frame)?;
        let prompt = Instant::now();
        let (mask, score) = engine.segment(&embedding, &[(x, y, true)])?;
        let decoder_ms = prompt.elapsed().as_millis();
        let cutout = extract(&frame, &mask).map_err(anyhow::Error::msg)?;
        std::fs::write(output, platform::png(&cutout)?)?;
        println!(
            "provider={} load_ms={} decoder_ms={} total_ms={} score={score:.3} output={}x{} area={}",
            engine.provider,
            loaded,
            decoder_ms,
            start.elapsed().as_millis(),
            cutout.width,
            cutout.height,
            mask.area()
        );
        return Ok(());
    }
    let smoke = args.iter().any(|s| s == "--smoke");
    if args
        .first()
        .is_some_and(|s| !matches!(s.as_str(), "--cpu" | "--once" | "--demo" | "--smoke"))
    {
        anyhow::bail!("Unknown command. Run objectlift --help");
    }
    let fixture = if let Some(i) = args.iter().position(|s| s == "--demo" || s == "--smoke") {
        let path = args
            .get(i + 1)
            .context("--demo/--smoke requires an image path")?;
        let image = image::open(path)?
            .resize(1280, 800, image::imageops::FilterType::Triangle)
            .to_rgba8();
        Some(
            Frame::new(image.width(), image.height(), image.into_raw())
                .map_err(anyhow::Error::msg)?,
        )
    } else {
        None
    };
    let mut settings =
        objectlift::settings::Settings::load(&objectlift::settings::Settings::path())?;
    if args.iter().any(|s| s == "--cpu") {
        settings.cpu_only = true;
    }
    objectlift::app::run(
        models,
        settings,
        args.iter().any(|s| s == "--once") || smoke || fixture.is_some(),
        fixture,
        smoke,
    )
}
