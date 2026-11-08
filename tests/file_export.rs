use objectlift::file_export::StagedFile;

#[test]
fn dropping_an_uncommitted_export_preserves_the_destination() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("existing.png");
    std::fs::write(&path, b"original").unwrap();
    let staged = StagedFile::write(&path, true, |file| {
        use std::io::Write;
        file.write_all(b"replacement")?;
        Ok(())
    })
    .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    drop(staged);
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn a_file_created_after_staging_is_not_overwritten_without_consent() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cutout.png");
    let staged = StagedFile::write(&path, false, |file| {
        file.write_all(b"cutout")?;
        Ok(())
    })
    .unwrap();
    std::fs::write(&path, b"new file").unwrap();
    assert!(staged.commit().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"new file");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn lossless_webp_round_trips_visible_colors_and_fractional_alpha() {
    use objectlift::{
        Frame,
        file_export::{Format, stage},
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cutout.webp");
    let frame = Frame::new(2, 1, vec![190, 13, 88, 96, 20, 100, 244, 255]).unwrap();
    stage(&path, false, Format::WebP, &frame, None, (0, 0))
        .unwrap()
        .commit()
        .unwrap();
    assert_eq!(image::open(path).unwrap().to_rgba8().into_raw(), frame.rgba);
}

#[test]
fn bundle_contains_only_the_aligned_source_crop_and_effective_alpha() {
    use objectlift::{
        Frame,
        file_export::{Format, stage},
    };
    use std::io::Read;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cutout.zip");
    let source = Frame::new(3, 1, vec![9, 8, 7, 255, 10, 20, 30, 192, 40, 50, 60, 255]).unwrap();
    let ready = Frame::new(1, 1, vec![200, 20, 30, 96]).unwrap();
    stage(&path, false, Format::Bundle, &ready, Some(&source), (1, 0))
        .unwrap()
        .commit()
        .unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    assert_eq!(zip.len(), 4);
    let mut member = |name| {
        let mut bytes = Vec::new();
        zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
        bytes
    };
    assert_eq!(
        image::load_from_memory(&member("source.png"))
            .unwrap()
            .to_rgba8()
            .into_raw(),
        [10, 20, 30, 192]
    );
    assert_eq!(
        image::load_from_memory(&member("alpha-mask.png"))
            .unwrap()
            .to_luma8()
            .into_raw(),
        [96]
    );
    assert_eq!(
        image::load_from_memory(&member("cutout.png"))
            .unwrap()
            .to_rgba8()
            .into_raw(),
        ready.rgba
    );
    let manifest: serde_json::Value = serde_json::from_slice(&member("manifest.json")).unwrap();
    assert_eq!(manifest["crop"]["x"], 1);
}

#[cfg(feature = "ag-psd")]
#[test]
fn psd_has_a_real_mask_recovered_rgb_and_a_hidden_source() {
    use objectlift::{
        Frame,
        file_export::{Format, stage},
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("editable.psd");
    let source = Frame::new(2, 1, vec![10, 20, 30, 192, 40, 50, 60, 255]).unwrap();
    let ready = Frame::new(2, 1, vec![200, 20, 30, 96, 0, 0, 0, 0]).unwrap();
    stage(&path, false, Format::Psd, &ready, Some(&source), (0, 0))
        .unwrap()
        .commit()
        .unwrap();
    let doc = ag_psd::read_psd(
        &std::fs::read(path).unwrap(),
        &ag_psd::psd::ReadOptions {
            use_image_data: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    let layers = doc.children.as_ref().unwrap();
    assert_eq!(layers.len(), 2);
    let visible = layers
        .iter()
        .find(|layer| layer.hidden != Some(true))
        .unwrap();
    let hidden = layers
        .iter()
        .find(|layer| layer.hidden == Some(true))
        .unwrap();
    assert_eq!(
        visible.image_data.as_ref().unwrap().data,
        [200, 20, 30, 255, 40, 50, 60, 255]
    );
    let mask = visible
        .additional_info
        .mask
        .as_ref()
        .unwrap()
        .image_data
        .as_ref()
        .unwrap();
    assert_eq!(
        mask.data.chunks_exact(4).map(|p| p[0]).collect::<Vec<_>>(),
        [96, 0]
    );
    assert_eq!(hidden.image_data.as_ref().unwrap().data, source.rgba);
    let merged = &doc.image_data.as_ref().unwrap().data;
    for (actual, expected) in merged.chunks_exact(4).zip(ready.rgba.chunks_exact(4)) {
        assert_eq!(actual[3], expected[3]);
        // The PSD merged preview stores white-matted 8-bit channels. Compare
        // the rendered contribution, where unmatting error is at most one level.
        for channel in 0..3 {
            assert!(
                (i32::from(actual[channel]) - i32::from(expected[channel])).abs()
                    * i32::from(expected[3])
                    <= 255
            );
        }
    }
}

#[test]
fn a_stale_background_job_never_publishes_its_file() {
    use objectlift::{
        Session,
        file_export::{FileJob, FileProgress},
    };
    use std::{
        io::Write,
        time::{Duration, Instant},
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cutout.png");
    let destination = path.clone();
    let mut session = Session::default();
    session.begin();
    let mut job = FileJob::start(session.id(), session.revision(), move || {
        StagedFile::write(&destination, false, |file| {
            file.write_all(b"obsolete")?;
            Ok(())
        })
    });
    session.revise();
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        match job.poll(&session) {
            FileProgress::Finished(None) => break,
            FileProgress::Pending => {}
            _ => panic!("obsolete job received approval"),
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!path.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[cfg(windows)]
#[test]
fn a_locked_destination_survives_failed_replacement() {
    use std::{io::Write, os::windows::fs::OpenOptionsExt};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("locked.png");
    std::fs::write(&path, b"original").unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    let staged = StagedFile::write(&path, true, |file| {
        file.write_all(b"replacement")?;
        Ok(())
    })
    .unwrap();
    assert!(staged.commit().is_err());
    drop(lock);
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
