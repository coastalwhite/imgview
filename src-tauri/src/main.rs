// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

const SUPPORTED_EXTENSIONS: &[&str] = &["svg", "png", "jpg", "jpeg", "gif", "webp", "pgm", "pbm", "ppm"];

fn main() {
    // WebKitGTK/GDK can fail to detect the display DPI under some Wayland /
    // XWayland / nested-compositor setups (no `Xft.dpi` X resource or XSETTINGS
    // daemon providing one) — GDK's resolution query then returns its "unknown"
    // sentinel, and WebKitGTK derives devicePixelRatio from it as roughly -1/96,
    // corrupting every getBoundingClientRect() in the webview (huge negative
    // "sizes" for every element). Forcing the X11 backend avoids this. Only
    // applied if the user hasn't already picked a backend themselves, and only
    // on Linux (GDK_BACKEND is meaningless elsewhere). Safe to mutate the
    // environment here: this is the first thing main() does, before Tauri/GTK
    // or any other thread has started.
    #[cfg(target_os = "linux")]
    if std::env::var_os("GDK_BACKEND").is_none() {
        unsafe {
            std::env::set_var("GDK_BACKEND", "x11");
        }
    }

    // WebKitGTK's threaded compositor (and the GL context it needs) has been
    // unconditionally on since 2.14, and is a meaningful chunk of cold-start
    // time for exactly this kind of short-lived single-window app — a
    // WebKitGTK developer has stated disabling it "gives much faster load
    // times" (https://github.com/kapouer/node-webkitgtk/issues/55). Software
    // rendering is a fine trade here: this is a small, mostly-static image
    // viewer, not something that needs GPU-accelerated compositing.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_COMPOSITING_MODE").is_none() {
        unsafe {
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        }
    }

    let path = match parse_args() {
        Ok(path) => path,
        Err(msg) => {
            eprintln!("error: {msg}");
            eprintln!("Usage: imgview <path-to-image>");
            std::process::exit(1);
        }
    };

    imgview_lib::run(path);
}

fn parse_args() -> Result<PathBuf, String> {
    let raw = std::env::args()
        .nth(1)
        .ok_or_else(|| "missing image path argument".to_string())?;

    let path = PathBuf::from(&raw);
    if !path.exists() {
        return Err(format!("file not found: {raw}"));
    }
    if !path.is_file() {
        return Err(format!("not a regular file: {raw}"));
    }

    let ext_ok = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| SUPPORTED_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false);
    if !ext_ok {
        return Err(format!(
            "unsupported or unrecognized file extension: {raw} (expected one of: {})",
            SUPPORTED_EXTENSIONS.join(", ")
        ));
    }

    std::fs::canonicalize(&path).map_err(|e| format!("could not resolve path {raw}: {e}"))
}
