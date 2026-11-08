use anyhow::{Context, Result};
use std::{
    fs::File,
    path::{Path, PathBuf},
};

/// Owns a temporary sibling until the active session authorizes publication.
pub struct StagedFile {
    temp: tempfile::NamedTempFile,
    destination: PathBuf,
    replace: bool,
}

impl StagedFile {
    pub fn write(
        destination: &Path,
        replace: bool,
        encode: impl FnOnce(&mut File) -> Result<()>,
    ) -> Result<Self> {
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut temp = tempfile::Builder::new()
            .prefix(".objectlift-")
            .suffix(".tmp")
            .tempfile_in(parent)
            .context("Creating temporary export file")?;
        encode(temp.as_file_mut())?;
        temp.as_file().sync_all().context("Flushing export file")?;
        Ok(Self {
            temp,
            destination: destination.to_owned(),
            replace,
        })
    }

    pub fn commit(self) -> Result<()> {
        if self.replace {
            self.temp.persist(&self.destination)
        } else {
            self.temp.persist_noclobber(&self.destination)
        }
        .map_err(|error| error.error)
        .context("Publishing export file")?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub enum Format {
    Png,
    WebP,
    Bundle,
    #[cfg(feature = "ag-psd")]
    Psd,
}
impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::WebP => "webp",
            Self::Bundle => "zip",
            #[cfg(feature = "ag-psd")]
            Self::Psd => "psd",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "Transparent PNG",
            Self::WebP => "Lossless WebP",
            Self::Bundle => "Source and mask bundle",
            #[cfg(feature = "ag-psd")]
            Self::Psd => "Editable PSD (experimental)",
        }
    }
    pub fn needs_source(self) -> bool {
        match self {
            Self::Bundle => true,
            #[cfg(feature = "ag-psd")]
            Self::Psd => true,
            _ => false,
        }
    }
}
pub fn stage(
    path: &Path,
    replace: bool,
    format: Format,
    ready: &crate::Frame,
    source: Option<&crate::Frame>,
    origin: (u32, u32),
) -> Result<StagedFile> {
    use image::ImageEncoder;
    use std::io::Write;
    crate::validate(ready.width, ready.height, ready.rgba.len(), 4).map_err(anyhow::Error::msg)?;
    StagedFile::write(path, replace, |file| {
        let mut buffer = std::io::BufWriter::new(file);
        match format {
            Format::Bundle => {
                return write_bundle(
                    buffer,
                    ready,
                    source.context("Select an object before exporting source pixels")?,
                    origin,
                );
            }
            #[cfg(feature = "ag-psd")]
            Format::Psd => write_psd(
                &mut buffer,
                ready,
                source.context("Select an object before exporting source pixels")?,
                origin,
            )?,
            Format::Png => image::codecs::png::PngEncoder::new(&mut buffer).write_image(
                &ready.rgba,
                ready.width,
                ready.height,
                image::ExtendedColorType::Rgba8,
            )?,
            Format::WebP => image::codecs::webp::WebPEncoder::new_lossless(&mut buffer)
                .write_image(
                    &ready.rgba,
                    ready.width,
                    ready.height,
                    image::ExtendedColorType::Rgba8,
                )?,
        }
        buffer.flush()?;
        Ok(())
    })
}

fn source_crop(source: &crate::Frame, ready: &crate::Frame, (x, y): (u32, u32)) -> Result<Vec<u8>> {
    let right = x.checked_add(ready.width).context("Crop width overflow")?;
    let bottom = y
        .checked_add(ready.height)
        .context("Crop height overflow")?;
    anyhow::ensure!(
        right <= source.width && bottom <= source.height,
        "Export crop is outside the source image"
    );
    let stride = source.width as usize * 4;
    let mut pixels = Vec::with_capacity(ready.rgba.len());
    for row in y..bottom {
        let start = row as usize * stride + x as usize * 4;
        let end = start + ready.width as usize * 4;
        pixels.extend_from_slice(
            source
                .rgba
                .get(start..end)
                .context("Invalid source image buffer")?,
        );
    }
    Ok(pixels)
}

fn write_bundle(
    output: impl std::io::Write + std::io::Seek,
    ready: &crate::Frame,
    source: &crate::Frame,
    origin: (u32, u32),
) -> Result<()> {
    use image::{ExtendedColorType, ImageEncoder};
    let crop = source_crop(source, ready, origin)?;
    let alpha: Vec<u8> = ready.rgba.chunks_exact(4).map(|pixel| pixel[3]).collect();
    let mut archive = zip::ZipWriter::new(output);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes, color) in [
        (
            "cutout.png",
            ready.rgba.as_slice(),
            ExtendedColorType::Rgba8,
        ),
        ("source.png", crop.as_slice(), ExtendedColorType::Rgba8),
        ("alpha-mask.png", alpha.as_slice(), ExtendedColorType::L8),
    ] {
        archive.start_file(name, options)?;
        image::codecs::png::PngEncoder::new(&mut archive).write_image(
            bytes,
            ready.width,
            ready.height,
            color,
        )?;
    }
    archive.start_file("manifest.json", options)?;
    serde_json::to_writer_pretty(
        &mut archive,
        &serde_json::json!({
            "version": 1,
            "crop": {"x": origin.0, "y": origin.1, "width": ready.width, "height": ready.height},
            "coordinates": "pixels relative to the captured frame",
            "source": "original cropped RGBA",
            "cutout": "straight RGBA with final foreground colors",
            "alpha_mask": "effective cutout alpha, including source transparency, 0 transparent to 255 opaque"
        }),
    )?;
    archive.finish()?.flush()?;
    Ok(())
}

#[cfg(feature = "ag-psd")]
fn write_psd(
    mut output: impl std::io::Write,
    ready: &crate::Frame,
    source: &crate::Frame,
    origin: (u32, u32),
) -> Result<()> {
    use ag_psd::{
        PixelData,
        psd::{Layer, LayerAdditionalInfo, LayerMaskData, Psd, WriteOptions},
    };
    let (width, height) = (ready.width, ready.height);
    anyhow::ensure!(
        u64::from(width) * u64::from(height) <= 8_388_608 && width <= 30_000 && height <= 30_000,
        "Editable PSD preview is limited to 8 Mi pixels and 30,000 pixels per side"
    );
    let crop = source_crop(source, ready, origin)?;
    let mut visible = crop.clone();
    let mut mask = Vec::with_capacity(ready.rgba.len());
    for (pixel, result) in visible.chunks_exact_mut(4).zip(ready.rgba.chunks_exact(4)) {
        let alpha = result[3];
        if alpha != 0 {
            pixel[..3].copy_from_slice(&result[..3]);
        }
        pixel[3] = 255;
        mask.extend_from_slice(&[alpha, alpha, alpha, 255]);
    }
    let pixels = |data| PixelData {
        width,
        height,
        data,
    };
    let document = Psd {
        width: width.into(),
        height: height.into(),
        image_data: Some(pixels(ready.rgba.clone())),
        children: Some(vec![
            Layer {
                image_data: Some(pixels(visible)),
                additional_info: LayerAdditionalInfo {
                    name: Some("Cutout".into()),
                    mask: Some(LayerMaskData {
                        image_data: Some(pixels(mask)),
                        default_color: Some(0.),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                ..Default::default()
            },
            Layer {
                hidden: Some(true),
                image_data: Some(pixels(crop)),
                additional_info: LayerAdditionalInfo {
                    name: Some("Original crop".into()),
                    ..Default::default()
                },
                ..Default::default()
            },
        ]),
        ..Default::default()
    };
    let bytes = std::panic::catch_unwind(|| {
        ag_psd::write_psd(
            &document,
            &WriteOptions {
                no_background: Some(true),
                ..Default::default()
            },
        )
    })
    .map_err(|_| anyhow::anyhow!("Experimental PSD encoder rejected the cutout"))?;
    output.write_all(&bytes)?;
    Ok(())
}

use std::sync::mpsc;

enum FileEvent {
    Prepared(mpsc::Sender<()>),
    Finished(Result<(), String>),
}

/// A single in-flight file export. Only the UI can approve its publication.
pub struct FileJob {
    session_id: u64,
    revision: u64,
    events: mpsc::Receiver<FileEvent>,
    publishing: bool,
}

pub enum FileProgress {
    Pending,
    Publishing,
    Finished(Option<Result<(), String>>),
}

impl FileJob {
    pub fn start(
        session_id: u64,
        revision: u64,
        prepare: impl FnOnce() -> Result<StagedFile> + Send + 'static,
    ) -> Self {
        let (tx, events) = mpsc::channel();
        std::thread::spawn(move || {
            let result = prepare().and_then(|staged| {
                let (approve, decision) = mpsc::channel();
                if tx.send(FileEvent::Prepared(approve)).is_ok() && decision.recv().is_ok() {
                    staged.commit()?;
                }
                Ok(())
            });
            let _ = tx.send(FileEvent::Finished(
                result.map_err(|error| format!("{error:#}")),
            ));
        });
        Self {
            session_id,
            revision,
            events,
            publishing: false,
        }
    }

    pub fn is_publishing(&self, session: &crate::Session) -> bool {
        self.publishing && session.accepts_result(self.session_id, self.revision)
    }

    pub fn poll(&mut self, session: &crate::Session) -> FileProgress {
        let active = session.accepts_result(self.session_id, self.revision);
        match self.events.try_recv() {
            Ok(FileEvent::Prepared(approve)) if active => match approve.send(()) {
                Ok(()) => {
                    self.publishing = true;
                    FileProgress::Publishing
                }
                Err(_) => FileProgress::Finished(Some(Err("Export worker stopped".into()))),
            },
            Ok(FileEvent::Prepared(_)) | Err(mpsc::TryRecvError::Empty) => FileProgress::Pending,
            Ok(FileEvent::Finished(result)) => FileProgress::Finished(active.then_some(result)),
            Err(mpsc::TryRecvError::Disconnected) => {
                FileProgress::Finished(active.then(|| Err("Export worker stopped".into())))
            }
        }
    }
}
