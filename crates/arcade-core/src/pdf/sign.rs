//! Sign a PDF with a picture of a signature or a typed name in a script
//! font, placed on chosen pages with an optional date line. This is a
//! visible signature, not a cryptographic one.

use super::{
    output_name, pdf_page_count, pdf_page_sizes, points_to_mm, qpdf_provider, run_qpdf,
    success_file, write_private,
};
use crate::{
    artifacts::publish_without_overwrite,
    grants::FileGrants,
    magick::{Magick, args, literal_text},
    tool_kit::{check_cancelled, number_in, option_bool, option_str},
};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult, ValueKind};
use printpdf::{
    BuiltinFont, Color, Mm, Op, PdfDocument, PdfFontHandle, PdfPage, PdfSaveOptions, Point, Pt,
    RawImage, RawImageData, RawImageFormat, Rgb, TextItem, XObjectTransform,
};
use std::{ffi::OsString, sync::atomic::AtomicBool};

const MAX_PAGES: u64 = 2000;
/// Script fonts tried in order for typed signatures.
const SCRIPT_FONTS: [&str; 3] = [
    "Z003-MediumItalic",
    "URW-Chancery-L-Medium-Italic",
    "DejaVu-Serif-Italic",
];

pub(super) fn sign(
    manifest: &ToolManifest,
    request: &ToolRequest,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let (pdf, signature_image) = match request.inputs.as_slice() {
        [pdf] => (pdf, None),
        [pdf, image] => (pdf, Some(image)),
        _ => {
            return Err(
                "Select the PDF, then (for a picture signature) the signature image".into(),
            );
        }
    };
    if pdf.kind != ValueKind::Artifact || pdf.mime != "file/pdf" {
        return Err("The first file must be the PDF to sign".into());
    }
    let source = grants
        .resolve(&pdf.value)
        .map_err(|error| error.to_string())?;
    let parent = source.parent().ok_or("Selected PDF has no parent folder")?;
    let temp = tempfile::Builder::new()
        .prefix(".arcade-pdf-")
        .tempdir_in(parent)
        .map_err(|error| format!("Cannot create a private PDF workspace: {error}"))?;
    let magick = Magick::discover()?;
    let ink = match option_str(request, "ink", "blue") {
        "blue" => "#1a3a8f",
        "black" => "#111111",
        other => return Err(format!("Unknown ink colour: {other}")),
    };

    // 1. A transparent, tightly trimmed signature PNG.
    match option_str(request, "mode", "typed") {
        "typed" => {
            let name = literal_text(option_str(request, "name", ""))
                .map_err(|_| "Type the name to sign with".to_string())?;
            let fonts = magick
                .run(temp.path(), args(["-list", "font"]), cancelled)
                .unwrap_or_default();
            let font = SCRIPT_FONTS.into_iter().find(|font| {
                fonts
                    .lines()
                    .any(|line| line.trim().strip_prefix("Font: ") == Some(font))
            });
            let mut command = args(["-background", "none", "-fill", ink, "-pointsize", "160"]);
            if let Some(font) = font {
                command.extend(args(["-font", font]));
            }
            command.push(format!("label:{name}").into());
            command.extend(args(["-trim", "+repage", "signature.png"]));
            magick.run(temp.path(), command, cancelled)?;
        }
        "image" => {
            let image = signature_image.ok_or("Select the signature image after the PDF")?;
            if image.kind != ValueKind::Artifact || image.mime != "file/image" {
                return Err("The second file must be an image of the signature".into());
            }
            let path = grants
                .resolve(&image.value)
                .map_err(|error| error.to_string())?;
            let format = ::image::ImageFormat::from_path(&path)
                .ok()
                .and_then(|format| format.extensions_str().first().copied())
                .unwrap_or("png");
            let staged = format!("signature-source.{format}");
            std::fs::copy(&path, temp.path().join(&staged)).map_err(|error| error.to_string())?;
            let mut command: Vec<OsString> =
                vec![format!("{staged}[0]").into(), "-auto-orient".into()];
            if option_bool(request, "removeWhite", true) {
                // Photos and scans of ink on paper: drop the paper.
                command.extend(args(["-fuzz", "18%", "-transparent", "white"]));
            }
            command.extend(args([
                "-trim",
                "+repage",
                "-resize",
                "1600x1600>",
                "signature.png",
            ]));
            magick.run(temp.path(), command, cancelled)?;
        }
        other => return Err(format!("Unknown signature type: {other}")),
    }
    let decoded = ::image::open(temp.path().join("signature.png"))
        .map_err(|error| format!("Could not read the signature image: {error}"))?
        .into_rgba8();
    let (pixel_width, pixel_height) = decoded.dimensions();
    if pixel_width < 4 || pixel_height < 4 {
        return Err("The signature image looks empty after removing its background".into());
    }

    // 2. Where it goes.
    let page_count = pdf_page_count(&source, cancelled)?;
    if page_count == 0 || page_count > MAX_PAGES {
        return Err(format!("Signing supports 1 to {MAX_PAGES} pages"));
    }
    let sizes = pdf_page_sizes(&source, page_count as u32, cancelled)?;
    let pages: Vec<bool> = match option_str(request, "pages", "last") {
        "last" => (0..page_count)
            .map(|index| index + 1 == page_count)
            .collect(),
        "first" => (0..page_count).map(|index| index == 0).collect(),
        "all" => vec![true; page_count as usize],
        "page" => {
            let wanted = number_in(
                request,
                "pageNumber",
                "Page number",
                Some(1.0),
                1.0..=page_count as f64,
            )?
            .round() as u64;
            (0..page_count).map(|index| index + 1 == wanted).collect()
        }
        other => return Err(format!("Unknown page choice: {other}")),
    };
    let width_share =
        number_in(request, "width", "Signature width", Some(28.0), 5.0..=90.0)? / 100.0;
    let position = option_str(request, "position", "bottom-right");
    let (custom_x, custom_y) = if position == "custom" {
        (
            number_in(
                request,
                "x",
                "Distance from the left",
                Some(60.0),
                0.0..=100.0,
            )? / 100.0,
            number_in(
                request,
                "y",
                "Distance from the top",
                Some(80.0),
                0.0..=100.0,
            )? / 100.0,
        )
    } else {
        (0.0, 0.0)
    };
    let date = option_bool(request, "date", false)
        .then(|| format!("Signed {}", chrono::Local::now().format("%-d %B %Y")));

    // 3. One overlay page per PDF page; only chosen pages get the signature.
    let mut document = PdfDocument::new("Arcade Box signature");
    let image = document.add_image(&RawImage {
        pixels: RawImageData::U8(decoded.into_raw()),
        width: pixel_width as usize,
        height: pixel_height as usize,
        data_format: RawImageFormat::RGBA8,
        tag: Vec::new(),
    });
    let mut overlay_pages = Vec::with_capacity(sizes.len());
    for (index, (page_width, page_height)) in sizes.iter().copied().enumerate() {
        check_cancelled(cancelled)?;
        let (page_width, page_height) = (page_width as f32, page_height as f32);
        let mut ops = Vec::new();
        if pages[index] {
            let margin = 36.0f32.min(page_width.min(page_height) / 10.0);
            let draw_width = page_width * width_share as f32;
            let draw_height = draw_width * pixel_height as f32 / pixel_width as f32;
            let date_space = if date.is_some() { 14.0 } else { 0.0 };
            let x = match position {
                "bottom-left" => margin,
                "bottom-center" => (page_width - draw_width) / 2.0,
                "bottom-right" | "top-right" => page_width - draw_width - margin,
                "custom" => (page_width * custom_x as f32 - draw_width / 2.0)
                    .clamp(0.0, page_width - draw_width),
                other => return Err(format!("Unknown position: {other}")),
            };
            let y = match position {
                "top-right" => page_height - draw_height - margin,
                "custom" => (page_height * (1.0 - custom_y as f32) - draw_height / 2.0)
                    .clamp(date_space, page_height - draw_height),
                _ => margin + date_space,
            };
            ops.extend([
                Op::SaveGraphicsState,
                Op::UseXobject {
                    id: image.clone(),
                    transform: XObjectTransform {
                        translate_x: Some(Pt(x)),
                        translate_y: Some(Pt(y)),
                        scale_x: Some(draw_width / pixel_width as f32),
                        scale_y: Some(draw_height / pixel_height as f32),
                        dpi: Some(72.0),
                        ..Default::default()
                    },
                },
                Op::RestoreGraphicsState,
            ]);
            if let Some(date) = &date {
                ops.extend([
                    Op::StartTextSection,
                    Op::SetTextCursor {
                        pos: Point {
                            x: Pt(x),
                            y: Pt(y - 11.0),
                        },
                    },
                    Op::SetFont {
                        font: PdfFontHandle::Builtin(BuiltinFont::Helvetica),
                        size: Pt(9.0),
                    },
                    Op::SetFillColor {
                        col: Color::Rgb(Rgb {
                            r: 0.25,
                            g: 0.25,
                            b: 0.25,
                            icc_profile: None,
                        }),
                    },
                    Op::ShowText {
                        items: vec![TextItem::Text(date.clone())],
                    },
                    Op::EndTextSection,
                ]);
            }
        }
        overlay_pages.push(PdfPage::new(
            Mm(points_to_mm(page_width as f64)),
            Mm(points_to_mm(page_height as f64)),
            ops,
        ));
    }
    document.with_pages(overlay_pages);
    let overlay = temp.path().join("signature-overlay.pdf");
    write_private(
        &overlay,
        &document.save(&PdfSaveOptions::default(), &mut Vec::new()),
    )?;
    let staged = temp.path().join("signed.pdf");
    let provider = qpdf_provider("pdf:structural")?;
    run_qpdf(
        &provider.executable_path,
        vec![
            source.as_os_str().to_os_string(),
            "--overlay".into(),
            overlay.as_os_str().to_os_string(),
            "--".into(),
            staged.as_os_str().to_os_string(),
        ],
        temp.path(),
        cancelled,
        "place the signature on the PDF",
    )?;
    let name = output_name(request, &source, "-signed.pdf")?;
    let final_path = publish_without_overwrite(&staged, parent, &name, cancelled)
        .map_err(|error| format!("Could not save the signed PDF: {error}"))?;
    let selected = grants
        .grant(&final_path)
        .map_err(|error| error.to_string())?;
    let signed = pages.iter().filter(|page| **page).count();
    let mut result = success_file(
        manifest,
        selected,
        &format!("Signed {signed} page{}", if signed == 1 { "" } else { "s" }),
    );
    result.warnings.push(
        "This is a visible signature image, not a certificate-based digital signature.".into(),
    );
    Ok(result)
}
