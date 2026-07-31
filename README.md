# ImgView

A very simple SVG/image viewer with pan & zoom made to be used as a CLI application. Uses [Tauri](https://tauri.app) and [SVG Pan/Zoom](https://github.com/bumbu/svg-pan-zoom).

> [!NOTE]
> This project was almost entirely AI generated, built with the help of an AI coding assistant.

## Usage

```sh
imgview <path-to-image>
```

Opens a window showing the image, fit to the window.

### Supported formats

`.svg`, `.png`, `.jpg` / `.jpeg`, `.gif`, `.webp`, and the Netpbm formats `.pgm`, `.pbm`, `.ppm` (decoded and re-encoded losslessly, since no browser engine understands them natively — zoom stays pixel-sharp instead of blurring, since these are usually small bitmaps).

## Controls

| Key                              | Action                                    |
| --------------------------------- | ------------------------------------------ |
| Left-click drag                   | Pan                                        |
| Mouse wheel                       | Zoom in/out toward the cursor              |
| Double-click                      | Zoom in                                    |
| <kbd>+</kbd> / <kbd>-</kbd>        | Zoom in/out, centered on the window        |
| <kbd>h</kbd> <kbd>j</kbd> <kbd>k</kbd> <kbd>l</kbd> or arrow keys | Pan (vim motions)     |
| <kbd>Space</kbd>                  | Reset to fit the current window            |
| <kbd>Shift</kbd> + <kbd>K</kbd>   | Toggle a red laser-pointer cursor          |
| <kbd>q</kbd>                      | Quit                                       |

## Building

Requires Rust and Tauri's [Linux prerequisites](https://v2.tauri.app/start/prerequisites/) (`webkit2gtk-4.1`, `gtk3`, `libsoup3`, `dbus`, and their headers).

```sh
cd src-tauri
cargo build --release        # plain binary at target/release/imgview
# or, for local development:
cargo tauri dev -- path/to/image.svg
```

### Nix

A flake is provided with a freestanding binary output (Linux only):

```sh
nix build          # result/bin/imgview
nix develop         # dev shell with all prerequisites
```

---

This project is for personal use only.
