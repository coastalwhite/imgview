use base64::Engine;
use serde::Serialize;
use std::path::PathBuf;
use tauri::Manager;

pub struct AppState {
    pub path: PathBuf,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ImagePayload {
    Svg { content: String },
    /// `pixelated` requests nearest-neighbor (crisp, non-blurred) scaling in
    /// the frontend instead of smooth interpolation — set for the Netpbm
    /// formats, which are typically small bitmaps where individual pixels
    /// should stay sharp when zoomed in. `width`/`height` are the image's
    /// natural pixel dimensions, so the frontend can wrap it in a correctly
    /// sized `<svg viewBox><image></svg>` (and hand it to svg-pan-zoom, same
    /// as real SVGs) synchronously, without waiting on an extra image-decode
    /// round trip just to learn its size.
    Raster {
        data_url: String,
        pixelated: bool,
        width: u32,
        height: u32,
    },
}

fn mime_for_extension(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

/// Formats no browser webview can decode natively. Decoded and re-encoded as
/// PNG server-side instead of just base64-wrapping the raw bytes.
const NETPBM_EXTENSIONS: &[&str] = &["pgm", "pbm", "ppm"];

/// Read the viewer's target file and return it in a form the frontend can
/// display directly: raw markup for SVG (injected inline so svg-pan-zoom can
/// attach to it), or a base64 data URL for raster formats.
#[tauri::command]
fn get_image(state: tauri::State<AppState>) -> Result<ImagePayload, String> {
    let path = &state.path;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if ext == "svg" {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        Ok(ImagePayload::Svg { content })
    } else if NETPBM_EXTENSIONS.contains(&ext.as_str()) {
        let image = image::open(path).map_err(|e| e.to_string())?;
        let (width, height) = (image.width(), image.height());
        let mut png_bytes = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(png_bytes);
        Ok(ImagePayload::Raster {
            data_url: format!("data:image/png;base64,{encoded}"),
            pixelated: true,
            width,
            height,
        })
    } else {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        // Header-only read (no full decode) — just enough to size the SVG
        // wrapper the frontend builds around this image.
        let (width, height) = image::image_dimensions(path).map_err(|e| e.to_string())?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        let data_url = format!("data:{};base64,{encoded}", mime_for_extension(&ext));
        Ok(ImagePayload::Raster {
            data_url,
            pixelated: false,
            width,
            height,
        })
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run(path: PathBuf) {
    let title = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "imgview".to_string());

    tauri::Builder::default()
        .manage(AppState { path })
        .setup(move |app| {
            if let Some(window) = app.get_webview_window("main") {
                window.set_title(&format!("imgview \u{2014} {title}"))?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_image])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
