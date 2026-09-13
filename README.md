# ImgView

A very simple SVG/image viewer with pan & zoom made to be used as a CLI application. Built with [GPUI](https://www.gpui.rs) and [GPUI Kit](https://gpui-kit.com), with SVG rasterization by [resvg](https://github.com/linebender/resvg).

> [!NOTE]
> This project was almost entirely AI generated, built with the help of an AI coding assistant.

## Usage

```sh
imgview <path-to-image>
```

Opens a window showing the image, fit to the window.

### Supported formats

`.svg`, `.png`, `.jpg` / `.jpeg`, `.gif`, `.webp`, and the Netpbm formats `.pgm`, `.pbm`, `.ppm`.

Every frame is rasterized at exactly the scale it is displayed at, rather than scaling one fixed-size texture on the GPU. So SVGs are re-rendered by resvg as you zoom and never soften, Netpbm bitmaps are nearest-neighbour sampled so their pixels stay square, and photos are properly filtered when the window shows them smaller than they are.

## Controls

| Key                              | Action                                    |
| --------------------------------- | ------------------------------------------ |
| Left-click drag                   | Pan                                        |
| Mouse wheel                       | Zoom in/out toward the cursor              |
| Double-click                      | Zoom in                                    |
| <kbd>+</kbd> / <kbd>-</kbd>        | Zoom in/out, centered on the window        |
| <kbd>h</kbd> <kbd>j</kbd> <kbd>k</kbd> <kbd>l</kbd> or arrow keys | Pan (vim motions)     |
| <kbd>Space</kbd>                  | Reset to fit the current window            |
| <kbd>Shift</kbd> + <kbd>K</kbd>   | Toggle a red laser-pointer dot             |
| <kbd>q</kbd>                      | Quit                                       |

The laser pointer hides the system cursor so only the red dot is visible. GPUI Kit's GPUI snapshot has no hidden cursor style, so `vendor/` carries a small patched copy of its Linux backend to add one — see `vendor/README.md`, including how to drop it once upstream gains `CursorStyle::None`. Cursor hiding is Wayland-only; under X11 the dot is drawn alongside the normal pointer.

## Building

Requires Rust 1.92 or newer (GPUI Kit's Linux dependency tree does not build on older toolchains), plus `fontconfig`, `freetype` and `libxkbcommon` with their headers. At runtime it needs a Vulkan driver, and Wayland or X11 client libraries.

```sh
cargo build --release      # target/release/imgview
cargo run -- path/to/image.svg
```

### Nix

A flake is provided with a freestanding binary output (Linux only):

```sh
nix build          # result/bin/imgview
nix develop         # dev shell with all prerequisites
```

---

This project is for personal use only.
