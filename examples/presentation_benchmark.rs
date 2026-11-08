use objectlift::{
    Frame,
    editor::View,
    presentation::{CachedGdi, Paint, Presenter},
};
use std::time::Instant;
use windows::{
    Win32::{Graphics::Gdi::*, UI::WindowsAndMessaging::*},
    core::w,
};
fn main() -> anyhow::Result<()> {
    let concurrent = std::env::args().any(|a| a == "--inference");
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let worker = if concurrent {
        let running = running.clone();
        Some(std::thread::spawn(move || {
            let mut model =
                objectlift::inference::Segmenter::load(std::path::Path::new("models"), true)
                    .unwrap();
            let image = image::open("artifacts/truck.jpg")
                .unwrap()
                .resize(900, 600, image::imageops::FilterType::Triangle)
                .to_rgba8();
            let frame = Frame::new(image.width(), image.height(), image.into_raw()).unwrap();
            let embedding = model.encode(&frame).unwrap();
            ready_tx.send(()).unwrap();
            let mut runs = 0;
            while running.load(std::sync::atomic::Ordering::Relaxed) {
                model.segment(&embedding, &[(375., 250., true)]).unwrap();
                runs += 1;
            }
            runs
        }))
    } else {
        None
    };
    if concurrent {
        ready_rx.recv()?;
    }

    let mut records = Vec::new();
    unsafe {
        for (w, h) in [(1920u32, 1080u32), (2560, 1440), (3840, 2160)] {
            let hwnd = CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("ObjectLift renderer verification"),
                WS_POPUP,
                0,
                0,
                w as i32,
                h as i32,
                None,
                None,
                None,
                None,
            )?;
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let dc = GetDC(Some(hwnd));
            let frame = Frame::new(w, h, [91, 105, 124, 255].repeat((w * h) as usize)).unwrap();
            let mut canvas = frame.rgba.clone();
            for p in canvas.chunks_exact_mut(4) {
                p.swap(0, 2);
            }
            let mut presenter = Presenter::default();
            let mut cached = CachedGdi::default();
            let paint = Paint {
                capture: 1,
                revision: 1,
                frame: &frame,
                canvas: &canvas,
                view: View::default(),
                opacity: 1.,
            };
            let start = Instant::now();
            assert!(presenter.draw(hwnd, paint));
            let cold = start.elapsed().as_secs_f64() * 1000.;
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w as i32,
                    biHeight: -(h as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            assert!(cached.draw(dc, paint));
            for backend in 0..4 {
                let mut times = Vec::new();
                for i in 0..30 {
                    let start = Instant::now();
                    if backend == 3 {
                        assert!(cached.draw(dc, paint));
                    } else if backend == 2 {
                        assert!(cached.draw(
                            dc,
                            Paint {
                                opacity: i as f32 / 30.,
                                ..paint
                            }
                        ));
                    } else if backend == 1 {
                        assert!(presenter.draw(
                            hwnd,
                            Paint {
                                opacity: i as f32 / 30.,
                                ..paint
                            }
                        ));
                    } else {
                        StretchDIBits(
                            dc,
                            0,
                            0,
                            w as i32,
                            h as i32,
                            0,
                            0,
                            w as i32,
                            h as i32,
                            Some(canvas.as_ptr().cast()),
                            &info,
                            DIB_RGB_COLORS,
                            SRCCOPY,
                        );
                        GdiFlush().ok()?;
                    }
                    times.push(start.elapsed().as_secs_f64() * 1000.);
                }
                times.sort_by(f64::total_cmp);
                records.push(serde_json::json!({"width":w,"height":h,"backend":if backend==3 {"Cached GDI static"}else if backend==2 {"Cached GDI fade"}else if backend==1 {"Direct2D"}else{"GDI"},"samples":30,"p50_ms":times[15],"p95_ms":times[28],"cold_d2d_upload_ms":cold}));
            }
            assert_eq!(cached.uploads, 1, "fades must reuse the same DIB buffers");
            assert_eq!(
                presenter.uploads, 1,
                "opacity changes must not upload or rebuild geometry"
            );
            presenter.reset();
            assert!(presenter.draw(
                hwnd,
                Paint {
                    capture: 2,
                    ..paint
                }
            ));
            assert_eq!(presenter.uploads, 2);
            ReleaseDC(Some(hwnd), dc);
            DestroyWindow(hwnd)?;
        }
    }
    running.store(false, std::sync::atomic::Ordering::Relaxed);
    let inference_runs = worker.map_or(0, |w| w.join().unwrap());
    println!(
        "{}",
        serde_json::json!({"records":records,"concurrent_inference":concurrent,"inference_runs":inference_runs,"scope":"Native draw API completion, 30 warm draws, concurrent inference flag reported. Not event-to-photon latency; windows beyond physical display may be clipped by compositor. Cache reuse and new capture reset asserted."})
    );
    Ok(())
}
