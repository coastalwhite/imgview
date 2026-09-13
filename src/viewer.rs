//! The window: one element that draws the image, and the pan/zoom state the
//! keyboard and mouse drive.
//!
//! Every frame is rasterized to the size of the viewport in device pixels and
//! then blitted, rather than handing the GPU one big texture to scale. That
//! means the image is resampled at the scale it is actually displayed at — an
//! SVG is re-rendered by resvg, so it never softens as you zoom in, and a
//! Netpbm bitmap is nearest-neighbour sampled, so its pixels stay square — and
//! the memory cost is one viewport, not one whole image multiplied by the zoom
//! level.
//!
//! The catch is that rasterizing is not free, and for SVG its cost grows with
//! how much detail lands on screen: this repository's own matplotlib
//! `chart.svg` takes about 10ms to fill a 1024x768 viewport when fitted, but
//! about 320ms once zoomed in. So the view renders *progressively* — while it
//! is being moved it is rasterized at a reduced resolution and stretched to
//! fill the window, and the full-resolution frame is drawn once it holds still.
//! Cost is very nearly linear in pixel count (measured on that same file: 320ms
//! at 1024x768 against 14ms at 256x192 of the same view), so the reduction
//! needed to hit the frame budget can be estimated from the previous frame.

use gpui_kit::component::{ActiveTheme, Root};
use gpui_kit::*;
use resvg::tiny_skia::{FilterQuality, IntSize, Pixmap, PixmapPaint, Transform};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::image_source::{load, Source};

/// The backdrop the image is letterboxed against. Deliberately a fixed neutral
/// dark rather than a theme color: it is the mat around the picture, and it
/// should not change what the picture looks like from one machine to the next.
const BACKDROP: u32 = 0x2b2b2b;

/// Zoom limits, as multiples of the fit-to-window scale. The webview version's
/// svg-pan-zoom `minZoom`/`maxZoom` were likewise relative to its initial fit,
/// so these are the same limits.
const MIN_ZOOM_FACTOR: f32 = 0.02;
const MAX_ZOOM_FACTOR: f32 = 100.0;

/// Zoom multiplier per step: one keypress, one wheel notch, or one
/// double-click. Matches svg-pan-zoom's `zoomScaleSensitivity: 0.3`.
const ZOOM_STEP: f32 = 1.3;

/// Scroll distance treated as one wheel notch, for platforms that report
/// smooth pixel deltas (touchpads) instead of discrete lines.
const PIXELS_PER_NOTCH: f32 = 50.0;

/// Screen pixels a keyboard pan moves, independent of the zoom level.
const PAN_STEP: f32 = 50.0;

/// Radius of the laser-pointer dot, in logical pixels.
const LASER_RADIUS: f32 = 7.0;

/// The cursor style that hides the pointer, so only the laser dot shows.
///
/// GPUI has no hidden cursor style of its own — official `gpui` has
/// `CursorStyle::None`, but GPUI Kit is built on `gpui-pre`, which predates it
/// — so `vendor/gpui-pre-linux` repurposes this variant to mean "hide". It is
/// named here rather than written inline so that swapping it for a real
/// `CursorStyle::None`, once upstream has one, is a one-line change. See
/// `vendor/README.md`.
const HIDDEN_CURSOR: CursorStyle = CursorStyle::IBeamCursorForVerticalLayout;

/// How long a frame may take to rasterize before a moving view drops to a
/// reduced resolution. Comfortably inside a 60Hz frame, leaving room for GPUI's
/// own work.
const INTERACTIVE_BUDGET: Duration = Duration::from_millis(8);

/// Coarsest reduction allowed while moving. Past this the preview is too soft to
/// aim with, and it is better to just miss the frame budget.
const MAX_DIVISOR: u32 = 4;

/// How long the view must hold still before the full-resolution frame replaces
/// the preview.
const SETTLE_DELAY: Duration = Duration::from_millis(100);

pub fn run(path: PathBuf) {
    let title = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "imgview".to_string());

    // Decode before the window exists. It is the only thing the window is for,
    // and a failure has to be reported inside it rather than by failing to
    // open it.
    let source = load(&path);

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1024.), px(768.)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some(format!("imgview \u{2014} {title}").into()),
                    ..Default::default()
                }),
                // The identifier the Tauri build used, so the installed
                // `.desktop` entry still matches this window.
                app_id: Some("com.imgview.app".to_string()),
                ..Default::default()
            };

            cx.open_window(options, |window, cx| {
                let viewer = cx.new(|cx| Viewer::new(source, window, cx));
                cx.new(|cx| Root::new(viewer, window, cx))
            })
            .expect("error while opening the imgview window");
        });
}

/// Maps image coordinates to viewport coordinates: `device = image * zoom +
/// origin`, both in device pixels.
#[derive(Clone, Copy, PartialEq)]
struct ViewTransform {
    /// Device pixels per image unit.
    zoom: f32,
    /// Where the image's top-left corner sits in the viewport.
    origin: (f32, f32),
    /// The zoom at which the whole image fits the viewport. Kept because the
    /// zoom limits are relative to it.
    fit_zoom: f32,
}

impl ViewTransform {
    fn fit(image: (f32, f32), viewport: (f32, f32)) -> Self {
        // Letterbox: the whole image visible, aspect ratio kept. svg-pan-zoom
        // spells this `fit()`; its `contain()` is the crop/cover scale, which
        // is not what this app ever wanted.
        let zoom = (viewport.0 / image.0).min(viewport.1 / image.1);
        Self {
            zoom,
            origin: (
                (viewport.0 - image.0 * zoom) / 2.0,
                (viewport.1 - image.1 * zoom) / 2.0,
            ),
            fit_zoom: zoom,
        }
    }

    /// Scales by `factor` about `anchor` (device pixels, viewport-relative), so
    /// whatever part of the image is under that point stays under it.
    fn zoom_by(&mut self, factor: f32, anchor: (f32, f32)) {
        let zoom = (self.zoom * factor).clamp(
            self.fit_zoom * MIN_ZOOM_FACTOR,
            self.fit_zoom * MAX_ZOOM_FACTOR,
        );
        let ratio = zoom / self.zoom;
        self.origin = (
            anchor.0 - (anchor.0 - self.origin.0) * ratio,
            anchor.1 - (anchor.1 - self.origin.1) * ratio,
        );
        self.zoom = zoom;
    }

    fn pan_by(&mut self, dx: f32, dy: f32) {
        self.origin = (self.origin.0 + dx, self.origin.1 + dy);
    }
}

/// A rasterized frame, and the viewport size, resolution divisor and view it
/// was rasterized for.
struct Rasterized {
    key: (u32, u32, u32, ViewTransform),
    image: Arc<RenderImage>,
}

/// A raster source resampled down to `scale`, reused across pans at the same
/// zoom level. See [`Viewer::rasterize`] for why minification needs this.
struct Downscaled {
    scale: f32,
    pixmap: Pixmap,
}

pub struct Viewer {
    source: Result<Source, String>,
    focus_handle: FocusHandle,
    view: Option<ViewTransform>,
    /// Viewport size the current `view` was fitted or re-clamped against, so a
    /// window resize can be noticed.
    viewport: Option<(u32, u32)>,
    rasterized: Option<Rasterized>,
    downscaled: Option<Downscaled>,
    /// Bounds and scale factor from the last paint. Input handlers need them to
    /// convert positions and to zoom about the window centre, and they only
    /// ever run after at least one paint.
    bounds: Bounds<Pixels>,
    scale_factor: f32,
    /// Where the pointer was when it last moved, in window coordinates. `None`
    /// until it has moved over the window at least once — the dot has nowhere
    /// to be drawn before that, and defaulting it to the origin would park it
    /// in the window's top-left corner.
    cursor: Option<Point<Pixels>>,
    /// Pointer position at the previous drag step, while a drag-pan is running.
    dragging: Option<Point<Pixels>>,
    laser_active: bool,
    /// Seconds per pixel from the last rasterization, used to predict what
    /// resolution the next one can afford.
    cost_per_pixel: Option<f32>,
    settle_task: Option<Task<()>>,
}

impl Viewer {
    fn new(source: Result<Source, String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        // Nothing else in the window takes focus, and the keyboard shortcuts
        // are the point of the app, so claim it up front.
        window.focus(&focus_handle, cx);

        Self {
            source,
            focus_handle,
            view: None,
            viewport: None,
            rasterized: None,
            downscaled: None,
            bounds: Bounds::default(),
            scale_factor: 1.0,
            cursor: None,
            dragging: None,
            laser_active: false,
            cost_per_pixel: None,
            settle_task: None,
        }
    }

    /// Viewport size in device pixels.
    fn viewport(&self) -> (f32, f32) {
        (
            f32::from(self.bounds.size.width) * self.scale_factor,
            f32::from(self.bounds.size.height) * self.scale_factor,
        )
    }

    /// A window position as device pixels relative to the viewport's top-left.
    fn device_point(&self, position: Point<Pixels>) -> (f32, f32) {
        (
            f32::from(position.x - self.bounds.origin.x) * self.scale_factor,
            f32::from(position.y - self.bounds.origin.y) * self.scale_factor,
        )
    }

    /// The resolution divisor to rasterize at: 1 for full resolution, more
    /// while the view is moving and a full-resolution frame would blow the
    /// budget. See this module's header for why cost is treated as linear in
    /// pixel count.
    fn divisor(&self, width: u32, height: u32) -> u32 {
        // Nothing measured yet, so start sharp and find out what it costs.
        let Some(cost_per_pixel) = self.cost_per_pixel else {
            return 1;
        };

        let budget = INTERACTIVE_BUDGET.as_secs_f32();
        let mut divisor = 1;
        while divisor < MAX_DIVISOR {
            let pixels = ((width / divisor) * (height / divisor)) as f32;
            if cost_per_pixel * pixels <= budget {
                break;
            }
            divisor += 1;
        }
        divisor
    }

    /// Re-rasterizes at full resolution once the view has held still for
    /// [`SETTLE_DELAY`], so a coarse preview is only ever what you see *while*
    /// moving. Assigning over the previous task cancels it, which is what makes
    /// this a debounce rather than a queue.
    fn schedule_settle(&mut self, cx: &mut Context<Self>) {
        self.settle_task = Some(cx.spawn(async move |viewer, cx| {
            cx.background_executor().timer(SETTLE_DELAY).await;
            viewer.update(cx, |_, cx| cx.notify()).ok();
        }));
    }

    /// Produces the frame for the current view, reusing the last one when
    /// nothing has moved.
    ///
    /// Runs during element prepaint, which is where the viewport's real size
    /// first becomes known.
    fn rasterize(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Arc<RenderImage>> {
        self.bounds = bounds;
        self.scale_factor = window.scale_factor();

        let Ok(source) = &self.source else {
            return None;
        };

        let (viewport_width, viewport_height) = self.viewport();
        let width = viewport_width.round() as u32;
        let height = viewport_height.round() as u32;
        if width == 0 || height == 0 {
            return None;
        }

        let image_size = source.size();
        let viewport = (width as f32, height as f32);
        let view = match self.view {
            // A resize keeps the user's zoom and pan — the webview version did
            // too, since svg-pan-zoom had no resize handler and `Space` was
            // what re-fitted. Only the limits are re-derived, so they stay
            // sensible relative to the new window.
            Some(mut view) if self.viewport != Some((width, height)) => {
                view.fit_zoom = ViewTransform::fit(image_size, viewport).fit_zoom;
                view
            }
            Some(view) => view,
            None => ViewTransform::fit(image_size, viewport),
        };
        self.view = Some(view);
        self.viewport = Some((width, height));

        // Reduced resolution is only ever for a view that is still moving. If
        // the last frame was rasterized for this very view, this pass is either
        // the first frame or the settle after movement stopped, and it gets
        // drawn sharp however long that takes.
        let moving = self
            .rasterized
            .as_ref()
            .is_some_and(|rasterized| rasterized.key != (width, height, rasterized.key.2, view));
        let divisor = if moving { self.divisor(width, height) } else { 1 };

        let key = (width, height, divisor, view);
        if let Some(rasterized) = &self.rasterized {
            if rasterized.key == key {
                return Some(rasterized.image.clone());
            }
        }

        // Fewer pixels covering the same view: scale the whole transform down
        // with the frame, and let the GPU stretch the result back over the
        // viewport when it is painted.
        let frame_width = (width / divisor).max(1);
        let frame_height = (height / divisor).max(1);
        let scaled = ViewTransform {
            zoom: view.zoom / divisor as f32,
            origin: (
                view.origin.0 / divisor as f32,
                view.origin.1 / divisor as f32,
            ),
            fit_zoom: view.fit_zoom,
        };

        let started = Instant::now();
        let pixmap = render(
            source,
            &mut self.downscaled,
            frame_width,
            frame_height,
            scaled,
        )?;
        self.cost_per_pixel =
            Some(started.elapsed().as_secs_f32() / (frame_width * frame_height) as f32);

        let image = Arc::new(into_render_image(pixmap));

        if let Some(previous) = self
            .rasterized
            .replace(Rasterized { key, image: image.clone() })
        {
            // Every frame is a fresh `RenderImage` with a fresh id, and the
            // texture atlas keys on that id — so without this the atlas would
            // gain an entry for every pan and zoom step and never lose one.
            window.drop_image(previous.image).ok();
        }

        if divisor > 1 {
            self.schedule_settle(cx);
        } else {
            self.settle_task = None;
        }

        Some(image)
    }

    fn refit(&mut self) {
        if let Ok(source) = &self.source {
            self.view = Some(ViewTransform::fit(source.size(), self.viewport()));
        }
    }

    fn zoom_at(&mut self, factor: f32, anchor: (f32, f32)) {
        if let Some(view) = &mut self.view {
            view.zoom_by(factor, anchor);
        }
    }

    fn zoom_centered(&mut self, factor: f32) {
        let (width, height) = self.viewport();
        self.zoom_at(factor, (width / 2.0, height / 2.0));
    }

    fn pan_by(&mut self, dx: f32, dy: f32) {
        if let Some(view) = &mut self.view {
            view.pan_by(dx, dy);
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();

        // Shift+K toggles the laser pointer. Checked first so plain `k` still
        // means "pan up", and so it works even when the file failed to load.
        if key == "k" && event.keystroke.modifiers.shift {
            self.laser_active = !self.laser_active;
            cx.notify();
            return;
        }

        if key == "q" {
            window.remove_window();
            return;
        }

        let step = PAN_STEP * self.scale_factor;
        match key {
            "space" => self.refit(),
            "=" | "+" => self.zoom_centered(ZOOM_STEP),
            "-" => self.zoom_centered(1.0 / ZOOM_STEP),
            "h" | "left" => self.pan_by(step, 0.0),
            "l" | "right" => self.pan_by(-step, 0.0),
            "k" | "up" => self.pan_by(0.0, step),
            "j" | "down" => self.pan_by(0.0, -step),
            _ => return,
        }
        cx.notify();
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);

        if event.click_count >= 2 {
            // The second press of a double-click. The first already started a
            // drag, so drop that and zoom toward the pointer instead.
            self.dragging = None;
            self.zoom_at(ZOOM_STEP, self.device_point(event.position));
            cx.notify();
        } else {
            self.dragging = Some(event.position);
        }
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.cursor = Some(event.position);
        let mut dirty = self.laser_active;

        // A drag runs from the press to the release, rather than from the
        // platform's `pressed_button` on each move: GPUI's Wayland backend
        // resets that field every time the pointer re-enters the surface, so
        // trusting it would end a pan part-way through the gesture.
        if let Some(previous) = self.dragging {
            self.pan_by(
                f32::from(event.position.x - previous.x) * self.scale_factor,
                f32::from(event.position.y - previous.y) * self.scale_factor,
            );
            self.dragging = Some(event.position);
            dirty = true;
        }

        if dirty {
            cx.notify();
        }
    }

    fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let notches = match event.delta {
            ScrollDelta::Lines(delta) => delta.y,
            ScrollDelta::Pixels(delta) => f32::from(delta.y) / PIXELS_PER_NOTCH,
        };
        if notches == 0.0 {
            return;
        }

        self.zoom_at(
            ZOOM_STEP.powf(notches),
            self.device_point(event.position),
        );
        cx.notify();
    }
}

impl Render for Viewer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div()
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(rgb(BACKDROP))
            .cursor(if self.laser_active {
                HIDDEN_CURSOR
            } else {
                CursorStyle::Arrow
            })
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| this.dragging = None),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| this.dragging = None),
            )
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel));

        match &self.source {
            Ok(_) => {
                let viewer = cx.entity().downgrade();
                root = root.child(
                    canvas(
                        move |bounds, window, cx| {
                            viewer
                                .update(cx, |viewer, cx| viewer.rasterize(bounds, window, cx))
                                .ok()
                                .flatten()
                        },
                        |bounds, image: Option<Arc<RenderImage>>, window, _| {
                            if let Some(image) = image {
                                // The frame covers exactly this rectangle, so a
                                // full-resolution one lands 1:1 and a reduced
                                // one is stretched back over it by the GPU.
                                window
                                    .paint_image(bounds, bounds, Corners::all(px(0.)), image, 0, false)
                                    .ok();
                            }
                        },
                    )
                    .size_full(),
                );
            }
            Err(message) => {
                root = root.child(
                    div()
                        .absolute()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p_4()
                        .text_center()
                        .text_color(cx.theme().danger)
                        .child(format!("Failed to load image: {message}")),
                );
            }
        }

        if let (true, Some(cursor)) = (self.laser_active, self.cursor) {
            root = root.child(laser_pointer(cursor - self.bounds.origin));
        }

        root
    }
}

/// A glowing red dot that follows the pointer, for pointing at things while
/// presenting. The real cursor is hidden underneath it via [`HIDDEN_CURSOR`],
/// so the dot replaces the pointer rather than sitting next to it — except
/// under X11, where hiding is not implemented and both are drawn.
fn laser_pointer(position: Point<Pixels>) -> impl IntoElement {
    div()
        .absolute()
        .left(position.x - px(LASER_RADIUS))
        .top(position.y - px(LASER_RADIUS))
        .size(px(LASER_RADIUS * 2.0))
        .rounded_full()
        .bg(rgb(0xe60000))
        .shadow(vec![
            BoxShadow::new(px(0.), px(0.), rgba(0xff0000cc).into())
                .blur_radius(px(10.))
                .spread_radius(px(3.)),
            BoxShadow::new(px(0.), px(0.), rgba(0xffffffb3).into()).blur_radius(px(3.)),
        ])
}

/// Draws the view into a fresh viewport-sized pixmap.
fn render(
    source: &Source,
    downscaled: &mut Option<Downscaled>,
    width: u32,
    height: u32,
    view: ViewTransform,
) -> Option<Pixmap> {
    // Starts fully transparent, so the backdrop behind it shows through
    // wherever the image is not — which is what letterboxing needs.
    let mut pixmap = Pixmap::new(width, height)?;

    match source {
        Source::Svg(tree) => {
            // resvg walks the whole tree but tiny_skia clips to the pixmap, so
            // only the visible part costs anything to fill, and it is
            // rasterized at precisely the on-screen scale.
            let transform = Transform::from_scale(view.zoom, view.zoom)
                .post_translate(view.origin.0, view.origin.1);
            resvg::render(tree, transform, &mut pixmap.as_mut());
        }
        Source::Raster { pixmap: original, pixelated } => {
            let (image_width, image_height) = (original.width() as f32, original.height() as f32);

            // Minifying needs the source resampled down first. tiny_skia
            // filters over a fixed 2x2/4x4 kernel, so once the scale drops
            // much below 1 it skips source pixels entirely and aliases badly;
            // `image`'s resize widens its kernel with the ratio and so averages
            // every pixel that contributes. Magnifying needs none of that, and
            // neither does SVG, which is rasterized at the final scale above.
            let sampled = if view.zoom < 1.0 {
                prepare_downscaled(original, downscaled, view.zoom)?
            } else {
                *downscaled = None;
                original
            };

            // One sampled pixel covers `image_width / sampled.width()` image
            // units, each of which is `zoom` device pixels across. Computed per
            // axis because rounding the resampled copy to whole pixels shifts
            // each axis by a little.
            let transform = Transform::from_scale(
                view.zoom * image_width / sampled.width() as f32,
                view.zoom * image_height / sampled.height() as f32,
            )
            .post_translate(view.origin.0, view.origin.1);

            let paint = PixmapPaint {
                quality: if *pixelated {
                    FilterQuality::Nearest
                } else if view.zoom >= 1.0 {
                    FilterQuality::Bicubic
                } else {
                    FilterQuality::Bilinear
                },
                ..Default::default()
            };
            pixmap.draw_pixmap(0, 0, sampled.as_ref(), &paint, transform, None);
        }
    }

    Some(pixmap)
}

/// Returns `source` resampled to roughly `scale`, reusing the cached copy when
/// the scale has not changed (so panning never pays for it).
fn prepare_downscaled<'a>(
    source: &'a Pixmap,
    cache: &'a mut Option<Downscaled>,
    scale: f32,
) -> Option<&'a Pixmap> {
    if cache.as_ref().map(|cached| cached.scale) != Some(scale) {
        let width = ((source.width() as f32 * scale).round() as u32).max(1);
        let height = ((source.height() as f32 * scale).round() as u32).max(1);

        let buffer: image::RgbaImage =
            image::ImageBuffer::from_raw(source.width(), source.height(), source.data().to_vec())?;
        // Triangle: its weights are all non-negative, so averaging valid
        // premultiplied pixels cannot produce an invalid one (a channel above
        // its own alpha), the way a Lanczos-style kernel's negative lobes can.
        let resized =
            image::imageops::resize(&buffer, width, height, image::imageops::FilterType::Triangle);
        let pixmap = Pixmap::from_vec(resized.into_raw(), IntSize::from_wh(width, height)?)?;

        *cache = Some(Downscaled { scale, pixmap });
    }

    cache.as_ref().map(|cached| &cached.pixmap)
}

/// Hands a finished pixmap to GPUI as a texture.
fn into_render_image(pixmap: Pixmap) -> RenderImage {
    let (width, height) = (pixmap.width(), pixmap.height());
    let mut data = pixmap.take();
    // GPUI uploads BGRA with straight alpha; tiny_skia produces premultiplied
    // RGBA. This is the same conversion GPUI's own SVG renderer does.
    for pixel in data.as_chunks_mut::<4>().0 {
        swap_rgba_pa_to_bgra(pixel);
    }

    let buffer = image::ImageBuffer::from_raw(width, height, data)
        .expect("pixmap data is always 4 bytes per pixel");
    RenderImage::new(vec![image::Frame::new(buffer)])
}
