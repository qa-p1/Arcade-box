//! Image layout tools built on ImageMagick: combine several images, add a
//! text or logo watermark, generate favicons and app icons, and make
//! passport/ID photos with a printable sheet.
//!
//! Every input is linked into a private workspace under a fixed name such as
//! `in-0.jpg`, so user paths (which ImageMagick would otherwise interpret for
//! `[frame]`, `format:` or `@file` syntax) never reach its command line.

use super::{ImageFormat, link_or_copy, rembg_cutout, safe_stem};
use crate::{
    Arcade,
    magick::{Magick, args, literal_text},
    provider::discover_background_removal_models,
    tool_kit::{number_in, option_bool, option_str, output_name, publish_file, success},
};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult, ToolValue, ValueKind};
use std::{ffi::OsString, path::Path, sync::atomic::AtomicBool};

const MAX_IMAGES: usize = 50;

pub(super) fn execute(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let magick = Magick::discover()?;
    let workspace = tempfile::tempdir_in(runtime.artifact_staging_root())
        .map_err(|error| format!("Cannot create a private image workspace: {error}"))?;
    let inputs = stage_inputs(request, runtime, workspace.path(), cancelled)?;
    let context = Context {
        manifest,
        request,
        runtime,
        cancelled,
        magick: &magick,
        dir: workspace.path(),
    };
    match manifest.id.as_str() {
        "arcade.image.combine" => combine(&context, &inputs),
        "arcade.image.watermark" => watermark(&context, &inputs),
        "arcade.image.favicon" => favicon(&context, &inputs),
        "arcade.image.passport" => passport(&context, &inputs),
        other => Err(format!("No image executor is registered for {other}")),
    }
}

struct Context<'a> {
    manifest: &'a ToolManifest,
    request: &'a ToolRequest,
    runtime: &'a Arcade,
    cancelled: &'a AtomicBool,
    magick: &'a Magick,
    dir: &'a Path,
}

impl Context<'_> {
    fn publish(&self, staged: &str, name: &str) -> Result<ToolValue, String> {
        publish_file(
            self.request,
            self.runtime,
            &self.dir.join(staged),
            name,
            self.cancelled,
        )
    }
}

/// A staged input: its fixed workspace name, format, and original stem.
struct Input {
    name: String,
    format: ImageFormat,
    stem: String,
}

fn stage_inputs(
    request: &ToolRequest,
    runtime: &Arcade,
    dir: &Path,
    cancelled: &AtomicBool,
) -> Result<Vec<Input>, String> {
    if request.inputs.is_empty() || request.inputs.len() > MAX_IMAGES {
        return Err(format!("Select between 1 and {MAX_IMAGES} images"));
    }
    request
        .inputs
        .iter()
        .enumerate()
        .map(|(index, input)| {
            if input.kind != ValueKind::Artifact || input.mime != "file/image" {
                return Err("Select images through Arcade Box".to_owned());
            }
            let source = runtime
                .grants()
                .resolve(&input.value)
                .map_err(|error| error.to_string())?;
            let format = ImageFormat::from_file(&source)?;
            let name = format!("in-{index}.{}", format.extension());
            link_or_copy(&source, &dir.join(&name), cancelled)?;
            Ok(Input {
                name,
                format,
                stem: safe_stem(&source),
            })
        })
        .collect()
}

fn background(request: &ToolRequest) -> Result<&'static str, String> {
    match option_str(request, "background", "white") {
        "white" => Ok("white"),
        "black" => Ok("black"),
        "transparent" => Ok("none"),
        other => Err(format!("Unknown background: {other}")),
    }
}

/// Output format and extension; JPEG cannot keep a transparent background.
fn output_format<'a>(request: &'a ToolRequest, default: &'a str) -> Result<&'a str, String> {
    match option_str(request, "format", default) {
        format @ ("jpg" | "png" | "webp") => Ok(format),
        other => Err(format!("Unknown format: {other}")),
    }
}

// ---------------------------------------------------------------------------
// Combine
// ---------------------------------------------------------------------------

fn combine(context: &Context, inputs: &[Input]) -> Result<ToolResult, String> {
    let request = context.request;
    if inputs.len() < 2 {
        return Err("Select at least two images to combine".into());
    }
    let layout = option_str(request, "layout", "vertical");
    let spacing = number_in(request, "spacing", "Spacing", Some(0.0), 0.0..=500.0)?.round() as u32;
    let fill = background(request)?;
    let format = output_format(request, "jpg")?;
    let fill = if format == "jpg" && fill == "none" {
        "white"
    } else {
        fill
    };
    let same_size = option_bool(request, "sameSize", true);
    let sizes = inputs
        .iter()
        .map(|input| {
            context
                .magick
                .size(context.dir, &input.name, context.cancelled)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut command: Vec<OsString> = inputs
        .iter()
        .map(|input| format!("{}[0]", input.name).into())
        .collect();
    let staged = format!("combined.{format}");
    match layout {
        "vertical" | "horizontal" => {
            let vertical = layout == "vertical";
            if same_size {
                // Match the smallest edge so nothing is upscaled.
                let edge = sizes
                    .iter()
                    .map(|(w, h)| if vertical { *w } else { *h })
                    .min()
                    .unwrap_or(1);
                command.extend([
                    "-resize".into(),
                    if vertical {
                        format!("{edge}x")
                    } else {
                        format!("x{edge}")
                    }
                    .into(),
                ]);
            }
            let (splice, gravity) = if vertical {
                (format!("0x{spacing}"), "north")
            } else {
                (format!("{spacing}x0"), "west")
            };
            command.extend(args(["-background", fill]));
            if spacing > 0 {
                command.extend([
                    "-gravity".into(),
                    gravity.into(),
                    "-splice".into(),
                    splice.clone().into(),
                ]);
            }
            command.extend(args([
                "-gravity",
                "center",
                if vertical { "-append" } else { "+append" },
            ]));
            if spacing > 0 {
                command.extend([
                    "-gravity".into(),
                    gravity.into(),
                    "-chop".into(),
                    splice.into(),
                ]);
            }
            command.extend(args(["+repage", &staged]));
            context
                .magick
                .run(context.dir, command, context.cancelled)?;
        }
        "grid" => {
            let columns =
                number_in(request, "columns", "Columns", Some(2.0), 1.0..=20.0)?.round() as u32;
            let cell = sizes
                .iter()
                .map(|(w, h)| (*w).max(*h))
                .min()
                .unwrap_or(1)
                .min(2000);
            let half = spacing / 2;
            let geometry = if same_size {
                format!("{cell}x{cell}+{half}+{half}")
            } else {
                format!("+{half}+{half}")
            };
            let mut montage: Vec<OsString> = vec!["montage".into()];
            montage.append(&mut command);
            montage.extend([
                "-tile".into(),
                format!("{columns}x").into(),
                "-geometry".into(),
                geometry.into(),
                "-background".into(),
                fill.into(),
                staged.clone().into(),
            ]);
            context
                .magick
                .run(context.dir, montage, context.cancelled)?;
        }
        other => return Err(format!("Unknown layout: {other}")),
    }
    let (width, height) = context
        .magick
        .size(context.dir, &staged, context.cancelled)?;
    let name = output_name(request, &format!("{}-combined.{format}", inputs[0].stem))?;
    let output = context.publish(&staged, &name)?;
    Ok(success(
        context.manifest,
        vec![output],
        Some(format!(
            "Combined {} images into {width}×{height}",
            inputs.len()
        )),
        vec![],
    ))
}

// ---------------------------------------------------------------------------
// Watermark
// ---------------------------------------------------------------------------

fn gravity(position: &str) -> Result<&'static str, String> {
    Ok(match position {
        "bottom-right" => "southeast",
        "bottom-left" => "southwest",
        "top-right" => "northeast",
        "top-left" => "northwest",
        "center" | "tiled" => "center",
        other => return Err(format!("Unknown position: {other}")),
    })
}

fn watermark(context: &Context, inputs: &[Input]) -> Result<ToolResult, String> {
    let request = context.request;
    let mode = option_str(request, "mode", "text");
    let (targets, logo) = match mode {
        "text" => (inputs, None),
        "logo" if inputs.len() >= 2 => (&inputs[..inputs.len() - 1], inputs.last()),
        "logo" => return Err("Select the photos first and the logo image last".into()),
        other => return Err(format!("Unknown watermark type: {other}")),
    };
    let position = option_str(request, "position", "bottom-right");
    let gravity = gravity(position)?;
    let opacity = number_in(request, "opacity", "Opacity", Some(50.0), 5.0..=100.0)? / 100.0;
    let scale = match option_str(request, "size", "medium") {
        "small" => 0.6,
        "medium" => 1.0,
        "large" => 1.6,
        other => return Err(format!("Unknown size: {other}")),
    };
    let text = if logo.is_none() {
        Some(literal_text(option_str(request, "text", ""))?)
    } else {
        None
    };
    let (red, green, blue) = if option_str(request, "color", "white") == "black" {
        (0, 0, 0)
    } else {
        (255, 255, 255)
    };
    let font = if text.is_some() {
        context.magick.bold_font(context.dir, context.cancelled)
    } else {
        None
    };
    let mut outputs = Vec::new();
    for (index, input) in targets.iter().enumerate() {
        let (width, height) = context
            .magick
            .size(context.dir, &input.name, context.cancelled)?;
        let short = width.min(height) as f64;
        let margin = (short * 0.03).round().max(4.0) as u32;
        let extension = input.format.writable().extension();
        let staged = format!("marked-{index}.{extension}");
        let mut command: Vec<OsString> = vec![format!("{}[0]", input.name).into()];
        let mark: Vec<OsString> = match (&text, logo) {
            (Some(text), _) => {
                let points = (short * 0.05 * scale).round().max(10.0);
                let mut mark = args(["-background", "none"]);
                if let Some(font) = font {
                    mark.extend(args(["-font", font]));
                }
                mark.push("-fill".into());
                mark.push(format!("rgba({red},{green},{blue},{opacity:.2})").into());
                mark.extend(args(["-stroke"]));
                mark.push(
                    format!(
                        "rgba({},{},{},{:.2})",
                        255 - red,
                        255 - green,
                        255 - blue,
                        opacity * 0.5
                    )
                    .into(),
                );
                mark.extend([
                    "-strokewidth".into(),
                    format!("{:.1}", (points / 28.0).max(1.0)).into(),
                ]);
                mark.extend(["-pointsize".into(), format!("{points}").into()]);
                mark.push(format!("label:{text}").into());
                if position == "tiled" {
                    mark.extend(args(["-rotate", "-30"]));
                }
                mark
            }
            (None, Some(logo)) => {
                let logo_width = (short * 0.2 * scale).round().max(16.0) as u32;
                let mut mark: Vec<OsString> = vec![
                    format!("{}[0]", logo.name).into(),
                    "-resize".into(),
                    format!("{logo_width}x").into(),
                ];
                mark.extend(args([
                    "-alpha",
                    "set",
                    "-channel",
                    "A",
                    "-evaluate",
                    "multiply",
                ]));
                mark.push(format!("{opacity:.2}").into());
                mark.extend(args(["+channel"]));
                mark
            }
            (None, None) => unreachable!(),
        };
        if position == "tiled" {
            // Build one tile with padding, then fill a transparent layer with it.
            command.push("(".into());
            command.extend(mark);
            command.extend([
                "-bordercolor".into(),
                "none".into(),
                "-border".into(),
                format!("{}", margin * 3).into(),
            ]);
            command.extend(args([
                "-write",
                "mpr:mark",
                "+delete",
                ")",
                "(",
                "+clone",
                "-alpha",
                "transparent",
                "-tile",
                "mpr:mark",
                "-draw",
                "color 0,0 reset",
                ")",
                "-composite",
            ]));
        } else {
            command.push("(".into());
            command.extend(mark);
            command.extend(args([")", "-gravity", gravity, "-geometry"]));
            command.push(if gravity == "center" {
                "+0+0".into()
            } else {
                format!("+{margin}+{margin}").into()
            });
            command.extend(args(["-composite"]));
        }
        if extension == "jpg" {
            command.extend(args(["-quality", "92"]));
        }
        command.push(staged.clone().into());
        context
            .magick
            .run(context.dir, command, context.cancelled)?;
        let name = if targets.len() == 1 {
            output_name(request, &format!("{}-watermarked.{extension}", input.stem))?
        } else {
            format!("{}-watermarked.{extension}", input.stem)
        };
        outputs.push(context.publish(&staged, &name)?);
    }
    let count = outputs.len();
    Ok(success(
        context.manifest,
        outputs,
        Some(format!(
            "Watermarked {count} image{}",
            if count == 1 { "" } else { "s" }
        )),
        vec![],
    ))
}

// ---------------------------------------------------------------------------
// Favicon and app icons
// ---------------------------------------------------------------------------

fn favicon(context: &Context, inputs: &[Input]) -> Result<ToolResult, String> {
    let request = context.request;
    let [input] = inputs else {
        return Err("Select one image (a square logo works best)".into());
    };
    let fill = background(request)?;
    let crop = option_str(request, "fit", "pad") == "crop";
    let set = option_str(request, "set", "web");
    let mut base = vec![OsString::from(format!("{}[0]", input.name))];
    base.extend(args(["-background", fill, "-gravity", "center", "-resize"]));
    base.push(if crop { "1024x1024^" } else { "1024x1024" }.into());
    base.extend(args(["-extent", "1024x1024", "+repage", "base.png"]));
    context.magick.run(context.dir, base, context.cancelled)?;
    let mut files: Vec<(String, String)> = Vec::new();
    let mut png = |size: u32, name: &str, flatten: bool| -> Result<(), String> {
        let mut command = args(["base.png"]);
        if flatten {
            command.extend(args([
                "-background",
                if fill == "none" { "white" } else { fill },
                "-flatten",
            ]));
        }
        command.extend([
            "-resize".into(),
            format!("{size}x{size}").into(),
            name.into(),
        ]);
        context
            .magick
            .run(context.dir, command, context.cancelled)?;
        files.push((name.to_owned(), name.to_owned()));
        Ok(())
    };
    match set {
        "web" => {
            png(180, "apple-touch-icon.png", true)?;
            png(192, "icon-192.png", false)?;
            png(512, "icon-512.png", false)?;
        }
        "app" => {
            for size in [16, 32, 48, 64, 128, 256, 512, 1024] {
                png(size, &format!("icon-{size}.png"), false)?;
            }
        }
        "ico" => {}
        other => return Err(format!("Unknown icon set: {other}")),
    }
    context.magick.run(
        context.dir,
        args([
            "base.png",
            "-define",
            "icon:auto-resize=16,24,32,48,64,256",
            "favicon.ico",
        ]),
        context.cancelled,
    )?;
    files.insert(0, ("favicon.ico".into(), "favicon.ico".into()));
    let mut outputs = files
        .iter()
        .map(|(staged, name)| context.publish(staged, name))
        .collect::<Result<Vec<_>, _>>()?;
    if set == "web" {
        let snippet = concat!(
            "<link rel=\"icon\" href=\"/favicon.ico\" sizes=\"any\">\n",
            "<link rel=\"apple-touch-icon\" href=\"/apple-touch-icon.png\">\n",
            "<link rel=\"manifest\" href=\"/site.webmanifest\">\n\n",
            "site.webmanifest:\n",
            "{\"icons\": [\n",
            "  {\"src\": \"/icon-192.png\", \"type\": \"image/png\", \"sizes\": \"192x192\"},\n",
            "  {\"src\": \"/icon-512.png\", \"type\": \"image/png\", \"sizes\": \"512x512\"}\n",
            "]}"
        );
        outputs.push(ToolValue::text(snippet, "text/html"));
    }
    Ok(success(
        context.manifest,
        outputs,
        Some(format!("Created {} icon files", files.len())),
        vec![],
    ))
}

// ---------------------------------------------------------------------------
// Passport / ID photos
// ---------------------------------------------------------------------------

/// Photo size in millimetres for each preset.
fn photo_size(preset: &str) -> Result<(f64, f64, &'static str), String> {
    Ok(match preset {
        "35x45" => (35.0, 45.0, "35×45 mm (UK, EU/Schengen, India, Australia)"),
        "51x51" => (50.8, 50.8, "2×2 in (US passport and visa, India visa)"),
        "50x70" => (50.0, 70.0, "50×70 mm (Canada)"),
        "33x48" => (33.0, 48.0, "33×48 mm (China)"),
        "35x35" => (35.0, 35.0, "35×35 mm"),
        other => return Err(format!("Unknown photo size: {other}")),
    })
}

fn passport(context: &Context, inputs: &[Input]) -> Result<ToolResult, String> {
    let request = context.request;
    let [input] = inputs else {
        return Err("Select one portrait photo".into());
    };
    let (width_mm, height_mm, label) = photo_size(option_str(request, "size", "35x45"))?;
    const DPI: f64 = 300.0;
    let width = (width_mm / 25.4 * DPI).round() as u32;
    let height = (height_mm / 25.4 * DPI).round() as u32;
    let mut source = format!("{}[0]", input.name);
    let mut warnings = Vec::new();
    let fill = match option_str(request, "background", "white") {
        "keep" => None,
        "white" => Some("white"),
        "light" => Some("#e8eef5"),
        other => return Err(format!("Unknown background: {other}")),
    };
    if let Some(fill) = fill {
        match discover_background_removal_models().into_iter().max_by_key(|model| model.asset.model_name == "u2net") {
            Some(model) => {
                rembg_cutout(&model, &context.dir.join(&input.name), &context.dir.join("cutout.png"), false, true, context.dir, context.cancelled)?;
                context.magick.run(
                    context.dir,
                    args(["cutout.png", "-background", fill, "-flatten", "+repage", "flat.png"]),
                    context.cancelled,
                )?;
                source = "flat.png".into();
            }
            None => warnings.push("Background replacement needs rembg with a U²-Net model, so the original background was kept.".into()),
        }
    }
    // Head-and-shoulders portraits keep their top; crop away the bottom.
    let anchor = if option_str(request, "anchor", "top") == "center" {
        "center"
    } else {
        "north"
    };
    let mut photo = vec![OsString::from(source)];
    photo.extend(args(["-auto-orient", "-resize"]));
    photo.push(format!("{width}x{height}^").into());
    photo.extend(args(["-gravity", anchor, "-extent"]));
    photo.push(format!("{width}x{height}").into());
    photo.extend(args([
        "+repage",
        "-units",
        "PixelsPerInch",
        "-density",
        "300",
        "-quality",
        "95",
        "photo.jpg",
    ]));
    context.magick.run(context.dir, photo, context.cancelled)?;
    let mut outputs = vec![context.publish(
        "photo.jpg",
        &output_name(request, &format!("{}-passport.jpg", input.stem))?,
    )?];
    if option_bool(request, "sheet", true) {
        // 6×4 inch print with as many copies as fit, each with a cut line.
        let (sheet_w, sheet_h, gap) = (1800u32, 1200u32, 24u32);
        let columns = ((sheet_w - gap) / (width + 2 + gap)).max(1);
        let rows = ((sheet_h - gap) / (height + 2 + gap)).max(1);
        let copies = columns * rows;
        let mut sheet = args(["montage"]);
        for _ in 0..copies {
            sheet.push("photo.jpg".into());
        }
        sheet.extend(args(["-bordercolor", "#bbbbbb", "-border", "1", "-tile"]));
        sheet.push(format!("{columns}x{rows}").into());
        sheet.extend([
            "-geometry".into(),
            format!("+{}+{}", gap / 2, gap / 2).into(),
        ]);
        sheet.extend(args(["-background", "white", "sheet-tiles.png"]));
        context.magick.run(context.dir, sheet, context.cancelled)?;
        let mut page = args([
            "sheet-tiles.png",
            "-gravity",
            "center",
            "-background",
            "white",
            "-extent",
        ]);
        page.push(format!("{sheet_w}x{sheet_h}").into());
        page.extend(args([
            "-units",
            "PixelsPerInch",
            "-density",
            "300",
            "-quality",
            "95",
            "sheet.jpg",
        ]));
        context.magick.run(context.dir, page, context.cancelled)?;
        outputs.push(context.publish(
            "sheet.jpg",
            &format!("{}-passport-sheet-6x4.jpg", input.stem),
        )?);
        warnings.push(format!(
            "Print the sheet at 6×4 in (10×15 cm) with no scaling to get {copies} photos."
        ));
    }
    warnings.push("Check your country's rules for head size and position; the photo is cropped, not face-aligned.".into());
    Ok(success(
        context.manifest,
        outputs,
        Some(format!(
            "Made a {label} photo ({width}×{height} px at 300 DPI)"
        )),
        warnings,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passport_presets_map_to_300_dpi_pixels() {
        let (w, h, _) = photo_size("35x45").unwrap();
        assert_eq!(
            ((w / 25.4 * 300.0).round(), (h / 25.4 * 300.0).round()),
            (413.0, 531.0)
        );
        assert_eq!(
            (photo_size("51x51").unwrap().0 / 25.4 * 300.0).round(),
            600.0
        );
    }
}
