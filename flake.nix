{
  description = "imgview: a Tauri v2 SVG/image viewer with pan and zoom";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-25.11";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    inputs@{ flake-parts, rust-overlay, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } (
      { ... }:
      {
        systems = [
          "x86_64-linux"
          "aarch64-linux"
          "x86_64-darwin"
          "aarch64-darwin"
        ];
        perSystem =
          {
            system,
            pkgs,
            lib,
            self',
            ...
          }:
          {
            _module.args.pkgs = import inputs.nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
            };

            # Freestanding `imgview` binary, buildable with `nix build`.
            # Linux-only for now: the webview backend (webkitgtk_4_1/GTK3) is
            # Linux-specific — Tauri uses WKWebView on macOS instead, which
            # would need a different (unresearched, untested) buildInputs set,
            # so this is intentionally not defined on the darwin systems above
            # rather than shipping something unverified.
            packages = lib.optionalAttrs pkgs.stdenv.isLinux {
              default = pkgs.rustPlatform.buildRustPackage {
                pname = "imgview";
                version = "0.1.0";

                # Whole repo, not just src-tauri/: tauri_build::build() reads
                # tauri.conf.json's `frontendDist: "../src"` and embeds those
                # files into the binary at *compile* time (there's no devUrl
                # configured, so this isn't just a dev-server convenience) —
                # the sibling src/ directory has to be present in the sandbox
                # for that relative path to resolve.
                src = ./.;
                cargoRoot = "src-tauri";
                buildAndTestSubdir = "src-tauri";
                cargoLock.lockFile = ./src-tauri/Cargo.lock;

                nativeBuildInputs = with pkgs; [
                  pkg-config
                  wrapGAppsHook4
                ];

                buildInputs = with pkgs; [
                  openssl
                  glib-networking
                  webkitgtk_4_1
                ];

                # Nix-packaged GTK/WebKit apps are notorious for multi-second
                # (sometimes tens-of-seconds) startup stalls if left unfixed:
                #  - fontconfig has no stable cache to trust, so it falls
                #    back to the host's /etc/fonts (version-mismatched
                #    against this Nix-built webkitgtk) and rescans every
                #    font directory from scratch on *every* launch — fixed by
                #    pinning a Nix-resolved fonts.conf via FONTCONFIG_FILE.
                #    wrapGAppsHook4 does not set this itself (confirmed from
                #    its source — it only covers GSettings/GIO/pixbuf/data
                #    dirs), so it has to be added explicitly here.
                #  - WebKitGTK's accessibility (ATK/AT-SPI) D-Bus handshake
                #    can stall for many seconds without a running a11y bus —
                #    short-circuited with NO_AT_BRIDGE.
                fontsConf = pkgs.makeFontsConf {
                  fontDirectories = [ pkgs.dejavu_fonts.minimal ];
                };
                preFixup = ''
                  gappsWrapperArgs+=(
                    --set FONTCONFIG_FILE "$fontsConf"
                    --set NO_AT_BRIDGE 1
                  )
                '';

                meta = {
                  description = "A minimal SVG/image viewer with pan and zoom";
                  mainProgram = "imgview";
                  platforms = lib.platforms.linux;
                };
              };
            };

            devShells.default =
              let
                dlopenLibraries = with pkgs; [
                  libxkbcommon

                  # GPU backend
                  vulkan-loader
                  # libGL

                  # Window system
                  wayland
                  # xorg.libX11
                  # xorg.libXcursor
                  # xorg.libXi
                  dbus
                ];
              in
              pkgs.mkShell {
                packages = with pkgs; [
                  webkitgtk_4_1
                  gtk3
                  gdk-pixbuf
                  pango
                  atk
                  cairo
                  glib
                  glib.dev
                  dbus
                  openssl
                  pkg-config
                  stdenv.cc.cc.lib
                  expat
                  fontconfig
                  freetype
                  wayland
                  libxkbcommon
                  vulkan-loader
                  cargo-tauri
                ];

                env.RUSTFLAGS = "-C link-arg=-Wl,-rpath,${pkgs.lib.makeLibraryPath dlopenLibraries}";
              };
          };
      }
    );
}
