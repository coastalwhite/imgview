mod image_source;
mod viewer;

use std::path::PathBuf;

use image_source::SUPPORTED_EXTENSIONS;

fn main() {
    let path = match parse_args() {
        Ok(path) => path,
        Err(message) => {
            eprintln!("error: {message}");
            eprintln!("Usage: imgview <path-to-image>");
            std::process::exit(1);
        }
    };

    viewer::run(path);
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

    let extension_ok = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| SUPPORTED_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()))
        .unwrap_or(false);
    if !extension_ok {
        return Err(format!(
            "unsupported or unrecognized file extension: {raw} (expected one of: {})",
            SUPPORTED_EXTENSIONS.join(", ")
        ));
    }

    std::fs::canonicalize(&path).map_err(|error| format!("could not resolve path {raw}: {error}"))
}
