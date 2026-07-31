const { invoke } = window.__TAURI__.core;
const { getCurrentWindow } = window.__TAURI__.window;

const MIN_ZOOM = 0.02;
const MAX_ZOOM = 100;
const PAN_STEP = 50; // screen pixels per keypress, independent of zoom level

let activeController = null;

let laserActive = false;
const laserPointerEl = document.getElementById("laser-pointer");

function setLaserActive(active) {
    laserActive = active;
    document.body.classList.toggle("laser-active", active);
}

document.addEventListener("mousemove", (e) => {
    if (!laserActive) return;
    laserPointerEl.style.left = `${e.clientX}px`;
    laserPointerEl.style.top = `${e.clientY}px`;
});

/// Resolves after the browser has completed a layout/paint pass. svg-pan-zoom
/// measures its target element's rendered box (getBoundingClientRect) the
/// moment it's constructed; calling it in the same tick as an `innerHTML`
/// injection can race a not-yet-laid-out element (0 or stale dimensions),
/// producing a degenerate transform. A couple of animation frames guarantees
/// layout has settled first. See https://github.com/bumbu/svg-pan-zoom/issues/353.
function waitForLayout() {
    return new Promise((resolve) => {
        requestAnimationFrame(() => requestAnimationFrame(resolve));
    });
}

/// Wraps a svg-pan-zoom instance with a small, stable interface for the
/// keydown handler below.
function wrapSvgPanZoom(panZoom) {
    return {
        fit: () => {
            // Re-measure in case the window was resized since load, then fit
            // (letterbox, whole image visible) and re-center.
            panZoom.resize();
            panZoom.fit();
            panZoom.center();
        },
        zoomIn: () => panZoom.zoomIn(),
        zoomOut: () => panZoom.zoomOut(),
        panBy: (dx, dy) => panZoom.panBy({ x: dx, y: dy }),
    };
}

function showError(container, message) {
    container.classList.add("viewer-error");
    container.textContent = message;
}

/// Raster formats aren't SVG, but wrapping one in a minimal synthetic SVG
/// (viewBox sized to its natural pixel dimensions, one <image> pointing at
/// the data URL) lets svg-pan-zoom drive pan/zoom for it exactly like a real
/// SVG file — one interaction implementation instead of two. `width`/`height`
/// come from the backend's (header-only, no full decode) dimension probe, so
/// this needs no async image-load step of its own before it can be sized.
function buildRasterSvg(payload) {
    const style = payload.pixelated ? ' style="image-rendering: pixelated"' : "";
    return (
        `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${payload.width} ${payload.height}">` +
        `<image width="${payload.width}" height="${payload.height}" href="${payload.data_url}"${style}/>` +
        `</svg>`
    );
}

async function init() {
    const container = document.getElementById("viewer");
    // Belt-and-suspenders alongside the CSS user-select/user-drag rules:
    // catches any native "drag this out" gesture that would otherwise fight
    // with our own click-drag panning.
    container.addEventListener("dragstart", (e) => e.preventDefault());

    let payload;
    try {
        payload = await invoke("get_image");
    } catch (err) {
        showError(container, `Failed to load image: ${err}`);
        return;
    }

    container.innerHTML = payload.kind === "svg" ? payload.content : buildRasterSvg(payload);
    const svgEl = container.querySelector("svg");
    if (!svgEl) {
        showError(container, "No <svg> root element found in file.");
        return;
    }

    // Let the browser lay out the freshly-injected SVG before svg-pan-zoom
    // measures it at construction time (see waitForLayout's doc comment).
    await waitForLayout();

    const panZoom = window.svgPanZoom(svgEl, {
        zoomScaleSensitivity: 0.3,
        minZoom: MIN_ZOOM,
        maxZoom: MAX_ZOOM,
        fit: true,
        center: true,
    });
    activeController = wrapSvgPanZoom(panZoom);
}

document.addEventListener("keydown", (e) => {
    // Shift+K toggles the laser pointer, independent of whether an image has
    // loaded yet. Plain "k" (no shift) still means "pan up", below.
    if (e.code === "KeyK" && e.shiftKey) {
        e.preventDefault();
        setLaserActive(!laserActive);
        return;
    }

    if (!activeController) return;

    switch (e.code) {
        case "Space":
            e.preventDefault();
            activeController.fit();
            break;
        case "Equal":
        case "NumpadAdd":
            e.preventDefault();
            activeController.zoomIn();
            break;
        case "Minus":
        case "NumpadSubtract":
            e.preventDefault();
            activeController.zoomOut();
            break;
        case "KeyH":
        case "ArrowLeft":
            e.preventDefault();
            activeController.panBy(PAN_STEP, 0);
            break;
        case "KeyL":
        case "ArrowRight":
            e.preventDefault();
            activeController.panBy(-PAN_STEP, 0);
            break;
        case "KeyK":
        case "ArrowUp":
            e.preventDefault();
            activeController.panBy(0, PAN_STEP);
            break;
        case "KeyJ":
        case "ArrowDown":
            e.preventDefault();
            activeController.panBy(0, -PAN_STEP);
            break;
        case "KeyQ":
            getCurrentWindow().close();
            break;
    }
});

window.addEventListener("DOMContentLoaded", init);
