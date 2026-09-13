{
  description = "imgview: a GPUI SVG/image viewer with pan and zoom";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-25.11";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    inputs@{ flake-parts, rust-overlay, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } (
      { self, ... }:
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
          let
            # nixpkgs-25.11 ships rustc 1.91, and GPUI Kit's Linux dependency
            # tree (oo7) refuses to build on anything before 1.92 — which is
            # what the rust-overlay input, already present but previously
            # unused by the package output, is for.
            rustToolchain = pkgs.rust-bin.stable.latest.default;
            rustPlatform = pkgs.makeRustPlatform {
              cargo = rustToolchain;
              rustc = rustToolchain;
            };
          in
          {
            _module.args.pkgs = import inputs.nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
            };

            # Freestanding `imgview` binary, buildable with `nix build`.
            # Linux-only: GPUI does support macOS, but it uses Metal and its own
            # text system there rather than the fontconfig/freetype/Vulkan stack
            # packaged below, so darwin would need a different (unresearched,
            # untested) set of inputs. Left undefined rather than shipping
            # something unverified.
            packages = lib.optionalAttrs pkgs.stdenv.isLinux {
              default = rustPlatform.buildRustPackage rec {
                pname = "imgview";
                version = "0.1.0";

                # So it shows up in application menus and as an "Open With"
                # option for its supported formats in file managers — imgview
                # itself needs a file argument (it's a viewer, not a picker),
                # so this is mainly useful for file-association, not for
                # launching with no arguments from a bare menu icon.
                desktopItem = pkgs.makeDesktopItem {
                  name = "imgview";
                  exec = "imgview %f";
                  icon = "image-x-generic";
                  desktopName = "ImgView";
                  genericName = "Image Viewer";
                  comment = "A minimal SVG/image viewer with pan and zoom";
                  categories = [
                    "Graphics"
                    "Viewer"
                  ];
                  mimeTypes = [
                    "image/svg+xml"
                    "image/png"
                    "image/jpeg"
                    "image/gif"
                    "image/webp"
                    "image/x-portable-graymap"
                    "image/x-portable-bitmap"
                    "image/x-portable-pixmap"
                    "image/x-portable-anymap"
                  ];
                };
                postInstall = ''
                  install -Dm644 ${desktopItem}/share/applications/*.desktop \
                    $out/share/applications/imgview.desktop
                '';

                src = ./.;
                cargoLock.lockFile = ./Cargo.lock;

                nativeBuildInputs = with pkgs; [ pkg-config ];

                # The libraries GPUI links against at build time: font discovery
                # (fontconfig), glyph rasterization (freetype, via font-kit) and
                # keyboard handling (libxkbcommon plus the libxkbcommon-x11 /
                # libxcb pair it needs, which the `xkbcommon` crate pulls in
                # with a plain `#[link]` rather than through a build script —
                # so pkg-config never reports them and they are easy to miss).
                buildInputs = with pkgs; [
                  fontconfig
                  freetype
                  libxkbcommon
                  xorg.libxcb
                ];

                # Everything else GPUI needs is dlopen'd by soname at runtime,
                # so it is invisible to the linker and has to be put on the
                # binary's RPATH explicitly or the app dies on first launch:
                # the Vulkan loader for wgpu, and the Wayland/XCB client
                # libraries for whichever display server is actually present.
                postFixup = ''
                  patchelf --add-rpath ${
                    lib.makeLibraryPath (
                      with pkgs;
                      [
                        vulkan-loader
                        wayland
                        libxkbcommon
                        xorg.libxcb
                      ]
                    )
                  } $out/bin/imgview
                '';

                # Deliberately *not* pinning FONTCONFIG_FILE the way the
                # WebKit-based build did. That pin bought a faster start by
                # restricting fontconfig to one bundled family, which is fine
                # for an app that only draws its own UI text — but this one
                # rasterizes SVG `<text>` through resvg against the system font
                # database, so hiding the installed fonts would silently change
                # what files render as.

                meta = {
                  description = "A minimal SVG/image viewer with pan and zoom";
                  mainProgram = "imgview";
                  platforms = lib.platforms.linux;
                };
              };
            };

            devShells.default =
              let
                # Same runtime-only libraries the package's postFixup adds,
                # so `cargo run` from the dev shell finds them too.
                dlopenLibraries = with pkgs; [
                  vulkan-loader
                  wayland
                  libxkbcommon
                  xorg.libxcb
                ];
              in
              pkgs.mkShell {
                packages = [ rustToolchain ] ++ (with pkgs; [
                  pkg-config
                  fontconfig
                  freetype
                  libxkbcommon
                  wayland
                  vulkan-loader
                  xorg.libxcb
                ]);

                env.RUSTFLAGS = "-C link-arg=-Wl,-rpath,${pkgs.lib.makeLibraryPath dlopenLibraries}";
              };
          };

        # `nixosModules`/`overlays`/etc. aren't per-system, so they live under
        # `flake.*` rather than `perSystem` (flake-parts merges this straight
        # into the flake's top-level outputs). Usage, in a NixOS config that
        # imports this flake:
        #   imports = [ imgview.nixosModules.default ];
        #   programs.imgview.enable = true;
        flake.nixosModules.default =
          {
            config,
            lib,
            pkgs,
            ...
          }:
          let
            cfg = config.programs.imgview;
          in
          {
            options.programs.imgview.enable = lib.mkEnableOption "imgview, a minimal CLI SVG/image viewer";

            config = lib.mkIf cfg.enable {
              # Indexes back into *this* flake's own `packages` output for the
              # importing system (built with our pinned nixpkgs/rust-overlay),
              # not something built against the importing config's own
              # nixpkgs — `pkgs` here is only used for its `.system` string.
              environment.systemPackages = [ self.packages.${pkgs.stdenv.hostPlatform.system}.default ];
            };
          };
      }
    );
}
