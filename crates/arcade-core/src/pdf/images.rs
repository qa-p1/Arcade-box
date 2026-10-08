//! Images to PDF without an external program. JPEGs are embedded as they are
//! (no re-encoding, so no quality loss); other formats are decoded and stored
//! losslessly with Flate. Transparency is flattened onto white.

use crate::tool_kit::check_cancelled;
use lopdf::{
    Document, Object, Stream,
    content::{Content, Operation},
    dictionary,
};
use std::{fs, io::Cursor, path::PathBuf, sync::atomic::AtomicBool};

pub(crate) const A4: (f32, f32) = (595.276, 841.89);
pub(crate) const LETTER: (f32, f32) = (612.0, 792.0);

/// Page layout, every length in PDF points.
pub(crate) struct Layout<'a> {
    /// A fixed portrait page size, or `None` for pages the size of each image.
    pub page: Option<(f32, f32)>,
    /// `auto`, `portrait` or `landscape` (fixed page sizes only).
    pub orientation: &'a str,
    /// `into`, `shrink`, `enlarge`, `fill` or `exact` (fixed page sizes only).
    pub fit: &'a str,
    pub margin: f32,
    /// Image pixels per inch: the image's size on the page.
    pub dpi: f32,
}

pub(crate) fn build(
    sources: &[PathBuf],
    layout: &Layout,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, String> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let mut kids = Vec::with_capacity(sources.len());
    for source in sources {
        check_cancelled(cancelled)?;
        let name = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let bytes = fs::read(source).map_err(|error| format!("Cannot read {name}: {error}"))?;
        let (image, width, height) =
            image_stream(&bytes).map_err(|error| format!("{name}: {error}"))?;
        let image_id = doc.add_object(image);
        let natural = (
            width as f32 * 72.0 / layout.dpi,
            height as f32 * 72.0 / layout.dpi,
        );
        let (page, placed, clip) = place(layout, natural);
        let mut operations = vec![Operation::new("q", vec![])];
        if let Some((x, y, w, h)) = clip {
            operations.extend([
                Operation::new("re", vec![x.into(), y.into(), w.into(), h.into()]),
                Operation::new("W", vec![]),
                Operation::new("n", vec![]),
            ]);
        }
        let (x, y, w, h) = placed;
        operations.extend([
            Operation::new(
                "cm",
                vec![w.into(), 0.into(), 0.into(), h.into(), x.into(), y.into()],
            ),
            Operation::new("Do", vec!["Im0".into()]),
            Operation::new("Q", vec![]),
        ]);
        let content = Content { operations }
            .encode()
            .map_err(|error| format!("Cannot lay out the PDF page: {error}"))?;
        let content_id = doc.add_object(Stream::new(dictionary! {}, content));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), page.0.into(), page.1.into()],
            "Contents" => content_id,
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image_id } },
        });
        kids.push(page_id.into());
    }
    let count = kids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|error| format!("Cannot write the PDF: {error}"))?;
    Ok(out)
}

/// An image XObject and its pixel size.
fn image_stream(bytes: &[u8]) -> Result<(Stream, u32, u32), String> {
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    if reader.format() == Some(image::ImageFormat::Jpeg) {
        if let Some((width, height, components)) = jpeg_frame(bytes) {
            let color_space = match components {
                1 => Some("DeviceGray"),
                3 => Some("DeviceRGB"),
                _ => None, // CMYK: decoded below.
            };
            if let Some(color_space) = color_space {
                let stream = Stream::new(
                    dictionary! {
                        "Type" => "XObject",
                        "Subtype" => "Image",
                        "Width" => width as i64,
                        "Height" => height as i64,
                        "ColorSpace" => color_space,
                        "BitsPerComponent" => 8,
                        "Filter" => "DCTDecode",
                    },
                    bytes.to_vec(),
                );
                return Ok((stream, width, height));
            }
        }
    }
    let decoded = reader
        .decode()
        .map_err(|error| format!("not a readable image ({error})"))?;
    let (width, height) = (decoded.width(), decoded.height());
    let (pixels, color_space) = if decoded.color().has_color() {
        let rgba = decoded.to_rgba8();
        let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
        for pixel in rgba.pixels() {
            let [r, g, b, a] = pixel.0;
            for channel in [r, g, b] {
                rgb.push(over_white(channel, a));
            }
        }
        (rgb, "DeviceRGB")
    } else {
        let la = decoded.to_luma_alpha8();
        (
            la.pixels().map(|p| over_white(p.0[0], p.0[1])).collect(),
            "DeviceGray",
        )
    };
    let mut stream = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => width as i64,
            "Height" => height as i64,
            "ColorSpace" => color_space,
            "BitsPerComponent" => 8,
        },
        pixels,
    );
    stream
        .compress()
        .map_err(|error| format!("cannot compress the image ({error})"))?;
    Ok((stream, width, height))
}

fn over_white(channel: u8, alpha: u8) -> u8 {
    ((channel as u32 * alpha as u32 + 255 * (255 - alpha as u32) + 127) / 255) as u8
}

/// Width, height and component count from a JPEG's start-of-frame segment.
fn jpeg_frame(bytes: &[u8]) -> Option<(u32, u32, u8)> {
    let mut i = 2;
    while i + 9 < bytes.len() {
        if bytes[i] != 0xFF {
            return None;
        }
        let marker = bytes[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        let length = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
            let height = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
            let width = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
            return Some((width, height, bytes[i + 9]));
        }
        i += 2 + length;
    }
    None
}

type Rect = (f32, f32, f32, f32);

/// The page size, where the image goes, and the clip for `fill`.
fn place(layout: &Layout, natural: (f32, f32)) -> ((f32, f32), Rect, Option<Rect>) {
    let m = layout.margin;
    let Some((short, long)) = layout.page else {
        let page = (natural.0 + 2.0 * m, natural.1 + 2.0 * m);
        return (page, (m, m, natural.0, natural.1), None);
    };
    let landscape = match layout.orientation {
        "landscape" => true,
        "portrait" => false,
        _ => natural.0 > natural.1,
    };
    let page = if landscape {
        (long, short)
    } else {
        (short, long)
    };
    let (box_w, box_h) = ((page.0 - 2.0 * m).max(1.0), (page.1 - 2.0 * m).max(1.0));
    let fit_scale = (box_w / natural.0).min(box_h / natural.1);
    let (w, h) = match layout.fit {
        "exact" => (box_w, box_h),
        "fill" => {
            let s = (box_w / natural.0).max(box_h / natural.1);
            (natural.0 * s, natural.1 * s)
        }
        "shrink" => {
            let s = fit_scale.min(1.0);
            (natural.0 * s, natural.1 * s)
        }
        "enlarge" => {
            let s = fit_scale.max(1.0);
            (natural.0 * s, natural.1 * s)
        }
        _ => (natural.0 * fit_scale, natural.1 * fit_scale),
    };
    let placed = ((page.0 - w) / 2.0, (page.1 - h) / 2.0, w, h);
    let clip = (layout.fit == "fill").then_some((m, m, box_w, box_h));
    (page, placed, clip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn layout(page: Option<(f32, f32)>, fit: &'static str) -> Layout<'static> {
        Layout {
            page,
            orientation: "auto",
            fit,
            margin: 0.0,
            dpi: 96.0,
        }
    }

    #[test]
    fn jpeg_is_embedded_without_reencoding_and_png_losslessly() {
        let dir = tempfile::tempdir().unwrap();
        let jpeg = dir.path().join("a.jpg");
        let png = dir.path().join("b.png");
        image::RgbImage::from_pixel(40, 20, image::Rgb([200, 10, 10]))
            .save(&jpeg)
            .unwrap();
        image::RgbaImage::from_pixel(10, 30, image::Rgba([0, 0, 0, 0]))
            .save(&png)
            .unwrap();
        let pdf = build(
            &[jpeg.clone(), png],
            &layout(None, "into"),
            &AtomicBool::new(false),
        )
        .unwrap();
        let doc = Document::load_mem(&pdf).unwrap();
        assert_eq!(doc.get_pages().len(), 2);
        let jpeg_bytes = fs::read(&jpeg).unwrap();
        let mut found_jpeg = false;
        for object in doc.objects.values() {
            if let Object::Stream(stream) = object {
                if stream.dict.get(b"Filter").ok() == Some(&Object::Name(b"DCTDecode".to_vec())) {
                    assert_eq!(stream.content, jpeg_bytes);
                    found_jpeg = true;
                }
            }
        }
        assert!(found_jpeg);
        // 40×20 px at 96 dpi is 30×15 pt.
        let first = doc.get_pages()[&1];
        let media = doc.get_dictionary(first).unwrap().get(b"MediaBox").unwrap();
        let media: Vec<f32> = media
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_float().unwrap())
            .collect();
        assert_eq!(media, vec![0.0, 0.0, 30.0, 15.0]);
    }

    #[test]
    fn fixed_pages_fit_center_and_turn_for_wide_images() {
        let (page, placed, clip) = place(&layout(Some(A4), "into"), (300.0, 150.0));
        assert_eq!(page, (A4.1, A4.0));
        assert!((placed.2 / placed.3 - 2.0).abs() < 1e-3);
        assert!(placed.2 <= A4.1 + 0.01 && placed.3 <= A4.0 + 0.01);
        assert!(clip.is_none());
        let (_, small, _) = place(&layout(Some(A4), "shrink"), (100.0, 100.0));
        assert_eq!((small.2, small.3), (100.0, 100.0));
        let (_, _, clip) = place(&layout(Some(LETTER), "fill"), (100.0, 300.0));
        assert!(clip.is_some());
    }

    #[test]
    fn white_background_replaces_transparency() {
        assert_eq!(over_white(0, 0), 255);
        assert_eq!(over_white(0, 255), 0);
    }
}
