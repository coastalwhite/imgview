//! Reading the file named on the command line into something that can be
//! rasterized repeatedly at whatever scale the current view needs.

use resvg::tiny_skia::{IntSize, Pixmap};
use resvg::usvg;
use std::path::Path;

pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "svg", "png", "jpg", "jpeg", "gif", "webp", "pgm", "pbm", "ppm",
];

/// Netpbm. These are typically small bitmaps where individual pixels are the
/// content, so magnifying them should stay blocky rather than smooth.
const PIXELATED_EXTENSIONS: &[&str] = &["pgm", "pbm", "ppm"];

pub enum Source {
    /// Resolution-independent: re-rasterized by resvg at the exact scale the
    /// view is showing, so curves and text stay sharp however far you zoom.
    Svg(Box<usvg::Tree>),
    /// A fixed grid of pixels, already premultiplied so tiny_skia can sample
    /// it directly.
    Raster { pixmap: Pixmap, pixelated: bool },
}

impl Source {
    /// The image's natural size, in the units the view transform maps from:
    /// CSS pixels for SVG, pixels for raster.
    pub fn size(&self) -> (f32, f32) {
        match self {
            Source::Svg(tree) => (tree.size().width(), tree.size().height()),
            Source::Raster { pixmap, .. } => (pixmap.width() as f32, pixmap.height() as f32),
        }
    }
}

/// Reads and decodes `path`. Errors come back as a message to display in the
/// window rather than a failure to open it at all.
pub fn load(path: &Path) -> Result<Source, String> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if extension == "svg" {
        load_svg(path)
    } else {
        load_raster(path, PIXELATED_EXTENSIONS.contains(&extension.as_str()))
    }
}

fn load_svg(path: &Path) -> Result<Source, String> {
    let data = std::fs::read(path).map_err(|error| error.to_string())?;

    let mut options = usvg::Options {
        // So a relative `<image href="logo.png">` resolves against the SVG's
        // own directory, the way a browser resolved it in the webview version.
        resources_dir: path.parent().map(|parent| parent.to_path_buf()),
        ..Default::default()
    };
    // usvg starts with an *empty* font database and silently drops any text it
    // cannot find a face for, so without this every `<text>` element in the
    // file renders as nothing at all.
    options.fontdb_mut().load_system_fonts();

    let tree = usvg::Tree::from_data(&data, &options)
        .map_err(|error| format!("could not parse SVG: {error}"))?;
    Ok(Source::Svg(Box::new(tree)))
}

fn load_raster(path: &Path, pixelated: bool) -> Result<Source, String> {
    let image = image::open(path)
        .map_err(|error| error.to_string())?
        .into_rgba8();
    let size = IntSize::from_wh(image.width(), image.height())
        .ok_or_else(|| "image has zero width or height".to_string())?;

    // tiny_skia works in premultiplied RGBA; the `image` crate hands back
    // straight alpha.
    let mut data = image.into_raw();
    for pixel in data.as_chunks_mut::<4>().0 {
        premultiply(pixel);
    }

    let pixmap = Pixmap::from_vec(data, size)
        .ok_or_else(|| "image is too large to rasterize".to_string())?;
    Ok(Source::Raster { pixmap, pixelated })
}

fn premultiply(pixel: &mut [u8]) {
    let alpha = pixel[3] as u32;
    if alpha == 255 {
        return;
    }
    for channel in &mut pixel[..3] {
        // +127 then /255 rounds to nearest, which keeps the result <= alpha and
        // so always a valid premultiplied pixel.
        *channel = ((*channel as u32 * alpha + 127) / 255) as u8;
    }
}
