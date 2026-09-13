# Vendored `gpui-pre-linux`

A copy of `gpui-pre-linux` 0.3.4 straight from crates.io, with one local patch,
wired in through `[patch.crates-io]` in the root `Cargo.toml`.

## Why

The laser pointer (<kbd>Shift</kbd>+<kbd>K</kbd>) needs to hide the system
cursor so that only the red dot is visible. GPUI can do this — hiding is a null
cursor surface on Wayland, and the code for it is already present — but there is
no way to ask for it from application code:

- Official [`gpui`](https://docs.rs/gpui) has a `CursorStyle::None` variant
  documented as "Hide the cursor". **GPUI Kit does not depend on that crate.** It
  depends on `gpui-pre`, longbridge's republished "snapshot of zed@6916400",
  whose `CursorStyle` has 21 variants and no `None`. 0.3.4 is the newest
  `gpui-pre`, so there is no version to upgrade to.
- `Platform::hide_cursor_until_mouse_moves()` exists and does exactly the right
  thing, but it is only reachable through `App::platform`, which is
  `pub(crate)`, and GPUI only triggers it behind `last_input_was_keyboard()` —
  and it restores on the next mouse move, which is precisely when a laser
  pointer needs to stay hidden.

Adding a real `CursorStyle::None` would mean forking `gpui-pre` as well (8 MB,
for one enum variant). Instead this patch repurposes an existing style.

## The patch

`CursorStyle::IBeamCursorForVerticalLayout` now means **hide the pointer**.

That variant is the stand-in because it is the one an application is least
likely to want by accident: it is the caret for vertical writing-mode text, and
nothing inside GPUI ever sets it. `imgview` refers to it through
`HIDDEN_CURSOR` in `src/viewer.rs`, never by name, so the choice is in one place
on each side.

Five edits:

| File | Change |
| --- | --- |
| `src/linux/platform.rs` | adds `is_hidden_cursor_style()` next to `cursor_style_to_icon_names()`, and maps the repurposed style to the default arrow icon |
| `src/linux/wayland/client.rs` | imports it, and guards the three sites that turn a style into a cursor |

The three guarded sites are `restore_cursor_after_hide()`, the `set_cursor_style()`
entry point, and the pointer `Enter` handler (so the cursor stays hidden when the
pointer leaves and re-enters the window). Each one calls
`wl_pointer.set_cursor(serial, None, 0, 0)` instead of the shape/icon path.
Every edit is marked with a `Local patch:` comment.

X11 hiding is *not* implemented. Its backend resolves styles through
`cursor_style_to_icon_names()`, which the Wayland path no longer reaches for
this style, so that lookup now returns the ordinary arrow — otherwise X11 would
draw a vertical-text caret, which is worse than what it did before. Under X11
the dot and the normal cursor are both visible, exactly as before this patch.

## Re-applying after a dependency bump

`[patch.crates-io]` is version-agnostic, so a `gpui-kit` upgrade will happily
keep using this 0.3.4 copy and silently pin the Linux backend to an old
revision. On any `gpui-kit` bump:

1. Check whether `gpui-pre` has gained a real hidden variant
   (`grep -rn "CursorStyle::None" ~/.cargo/registry/src/*/gpui-pre-*/src`). If it
   has, **delete this directory**, drop the `[patch.crates-io]` section, and set
   `HIDDEN_CURSOR` in `src/viewer.rs` to `CursorStyle::None`.
2. Otherwise re-vendor and re-apply: copy the new `gpui-pre-linux` from
   `~/.cargo/registry/src/*/`, then redo the four edits above — `grep -n
   "Local patch"` on the old copy lists every one of them.
