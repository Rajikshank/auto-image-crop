use objectlift::{Frame, platform};
#[test]
fn clipboard_formats_round_trip_alpha_and_channel_order() {
    let frame = Frame::new(2, 1, vec![10, 20, 30, 128, 40, 50, 60, 255]).unwrap();
    let png = platform::png(&frame).unwrap();
    let decoded = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(decoded.into_raw(), frame.rgba);
    let dib = platform::dibv5(&frame);
    assert_eq!(&dib[124..], &[15, 10, 5, 128, 60, 50, 40, 255]);
    assert_eq!(
        u32::from_le_bytes(dib[52..56].try_into().unwrap()),
        0xff000000
    );
}

#[test]
fn bitmap_fallback_uses_valid_premultiplied_alpha_while_png_stays_straight() {
    let frame = Frame::new(2, 1, vec![240, 120, 60, 128, 255, 255, 255, 0]).unwrap();
    let bitmap = platform::dibv5(&frame);
    assert_eq!(&bitmap[124..], &[30, 60, 120, 128, 0, 0, 0, 0]);
    assert!(
        bitmap[124..]
            .chunks_exact(4)
            .all(|p| p[..3].iter().all(|&c| c <= p[3]))
    );
    assert_eq!(
        image::load_from_memory(&platform::png(&frame).unwrap())
            .unwrap()
            .to_rgba8()
            .into_raw(),
        frame.rgba
    );
}

#[test]
fn saving_prepared_png_preserves_transparency_without_a_white_matte() {
    let frame = Frame::new(2, 1, vec![240, 120, 60, 128, 0, 0, 0, 0]).unwrap();
    let path = std::env::temp_dir().join(format!(
        "objectlift-alpha-{}-{}.png",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    platform::prepare(&frame).unwrap().save_png(&path).unwrap();
    let decoded = image::open(&path).unwrap().to_rgba8();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(decoded.into_raw(), frame.rgba);
}
