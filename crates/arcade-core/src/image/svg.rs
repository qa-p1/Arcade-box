//! SVG sources for the raster image tools. resvg renders them in-process,
//! with no system library, and the result goes through the normal pipeline
//! as a PNG with its transparency kept.

use std::path::Path;

/// The longest side of an SVG rendered for Convert when no size is given.
pub(super) const DEFAULT_SIZE: u32 = 512;
/// Large enough for the biggest icon Favicon makes (1024 px).
pub(super) const ICON_SIZE: u32 = 1024;
const MAX_SIZE: u32 = 8192;

/// Renders `source` so its longest side is `longest_side` pixels and writes
/// it to `output` as PNG. Returns the rendered width and height.
pub(super) fn rasterize(
    source: &Path,
    longest_side: u32,
    output: &Path,
) -> Result<(u32, u32), String> {
    if !(1..=MAX_SIZE).contains(&longest_side) {
        return Err(format!("SVG size must be between 1 and {MAX_SIZE} pixels"));
    }
    let data = std::fs::read(source).map_err(|error| format!("Couldn't read the SVG: {error}"))?;
    let tree = resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default())
        .map_err(|error| format!("Couldn't read the SVG: {error}"))?;
    let size = tree.size();
    let scale = longest_side as f32 / size.width().max(size.height());
    let width = ((size.width() * scale).round() as u32).max(1);
    let height = ((size.height() * scale).round() as u32).max(1);
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(width, height).ok_or("The SVG is too large to render")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // tiny-skia keeps premultiplied alpha; PNG wants straight alpha.
    let pixels = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect();
    ::image::RgbaImage::from_raw(width, height, pixels)
        .ok_or("Couldn't render the SVG")?
        .save_with_format(output, ::image::ImageFormat::Png)
        .map_err(|error| format!("Couldn't write the rendered SVG: {error}"))?;
    Ok((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128"><circle cx="64" cy="64" r="40" fill="#00aaee"/></svg>"##;

    #[test]
    fn renders_at_the_requested_size_and_keeps_transparency() {
        let dir = tempfile::tempdir().unwrap();
        let svg = dir.path().join("icon.svg");
        std::fs::write(&svg, ICON).unwrap();
        let png = dir.path().join("icon.png");
        assert_eq!(rasterize(&svg, 512, &png).unwrap(), (512, 512));
        let image = ::image::open(&png).unwrap().to_rgba8();
        assert_eq!(image.dimensions(), (512, 512));
        assert_eq!(
            image.get_pixel(0, 0)[3],
            0,
            "the corner outside the circle stays transparent"
        );
        assert_eq!(image.get_pixel(256, 256).0, [0x00, 0xaa, 0xee, 255]);
    }

    #[test]
    fn keeps_the_aspect_ratio_on_the_longest_side() {
        let dir = tempfile::tempdir().unwrap();
        let svg = dir.path().join("wide.svg");
        std::fs::write(
            &svg,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"/>"#,
        )
        .unwrap();
        assert_eq!(
            rasterize(&svg, 400, &dir.path().join("wide.png")).unwrap(),
            (400, 200)
        );
    }

    #[test]
    fn refuses_text_that_is_not_svg() {
        let dir = tempfile::tempdir().unwrap();
        let svg = dir.path().join("broken.svg");
        std::fs::write(&svg, "not svg").unwrap();
        assert!(rasterize(&svg, 64, &dir.path().join("broken.png")).is_err());
    }
}
