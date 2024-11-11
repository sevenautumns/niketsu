{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    utils.url = "github:numtide/flake-utils";
    devshell.url = "github:numtide/devshell";
    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";
    naersk.url = "github:nix-community/naersk";
    naersk.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { nixpkgs, devshell, utils, fenix, naersk, ... }:
    utils.lib.eachSystem [ "aarch64-linux" "x86_64-linux" ] (system:
      let
        lib = nixpkgs.lib;
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ devshell.overlays.default ];
        };
        static-rust-target = pkgs.pkgsStatic.targetPlatform.rust.rustcTarget;
        windows-target = pkgs.pkgsCross.mingwW64.targetPlatform.rust.rustcTarget;
        rust-toolchain = with fenix.packages.${system};
          combine [
            stable.rustc
            stable.cargo
            stable.clippy
            latest.rustfmt
            targets.${windows-target}.stable.rust-std
            targets.${static-rust-target}.stable.rust-std
          ];
        naersk-lib = (naersk.lib.${system}.override {
          cargo = rust-toolchain;
          rustc = rust-toolchain;
        });
        C_INCLUDE_PATH = lib.makeSearchPathOutput "dev" "include" (with pkgs; [
          xorg.libX11
          mpv-unwrapped
          fontconfig
          freetype
          expat
          musl
        ]);
        LD_LIBRARY_PATH = lib.makeLibraryPath [ pkgs.libclang.lib pkgs.mpv ];
        libraries = with pkgs; [
          mpv-unwrapped
          xorg.libX11
          xorg.libXcursor
          xorg.libXi
          xorg.libXrandr
          expat
          openssl
          freetype
          fontconfig
          vulkan-loader
          wayland
          wayland-protocols
          libxkbcommon
        ];
        VERSION = (with builtins;
          (fromTOML (readFile ./client/Cargo.toml))).package.version;
        LIBRARY_PATH = lib.makeLibraryPath libraries;
        PKG_CONFIG_PATH = lib.makeSearchPathOutput "dev" "lib/pkgconfig"
          (with pkgs; [ expat fontconfig freetype openssl ]);
        LIBCLANG_PATH = lib.makeLibraryPath [ pkgs.llvmPackages.libclang.lib ];
        BINDGEN_EXTRA_LANG_ARGS = p:
          "${builtins.readFile "${p.stdenv.cc}/nix-support/libc-crt1-cflags"} ${
            builtins.readFile "${p.stdenv.cc}/nix-support/libc-cflags"
          } ${builtins.readFile "${p.stdenv.cc}/nix-support/cc-cflags"} ${
            builtins.readFile "${p.stdenv.cc}/nix-support/libcxx-cxxflags"
          } -idirafter ${pkgs.libiconv}/include ${
            lib.optionalString p.stdenv.cc.isClang
            "-idirafter ${p.stdenv.cc.cc}/lib/clang/${
              lib.getVersion p.stdenv.cc.cc
            }/include"
          } ${
            lib.optionalString p.stdenv.cc.isGNU
            "-isystem ${p.stdenv.cc.cc}/include/c++/${
              lib.getVersion p.stdenv.cc.cc
            } -isystem ${p.stdenv.cc.cc}/include/c++/${
              lib.getVersion p.stdenv.cc.cc
            }/${p.stdenv.hostPlatform.config} -idirafter ${p.stdenv.cc.cc}/lib/gcc/${p.stdenv.hostPlatform.config}/${
              lib.getVersion p.stdenv.cc.cc
            }/include"
          }";
        WINDOWS_MPV_SOURCE = pkgs.stdenv.mkDerivation
          {
            name = "mpv-windows";
            src = pkgs.fetchurl {
              url =
                "https://altushost-swe.dl.sourceforge.net/project/mpv-player-windows/libmpv/mpv-dev-x86_64-v3-20241103-git-42ff6f9.7z";
              sha256 = "sha256-g/0Sgco0LCKvfQvtclG2v9XDj+SB78dH7y48j2qfLQ0=";
            };
            unpackCmd = ''
              ${pkgs.p7zip}/bin/7z x $curSrc
              mkdir $out
              cp -r * $out/
            '';
          };
      in
      rec {
        packages = {
          default = packages.niketsu-client;
          niketsu-client = naersk-lib.buildPackage rec {
            inherit LIBCLANG_PATH;
            name = "niketsu";
            version = VERSION;
            root = ./.;
            cargoBuildOptions = x: x ++ [ "--package" name ];
            cargoTestOptions = x: x ++ [ "--package" name ];
            nativeBuildInputs = with pkgs; [ cmake pkg-config ] ++ libraries;
            buildInputs = with pkgs; [ yt-dlp ];
            BINDGEN_EXTRA_CLANG_ARGS = BINDGEN_EXTRA_LANG_ARGS pkgs;
            C_INCLUDE_PATH = "$C_INCLUDE_PATH:${pkgs.mpv}/include";
          };
          niketsu-client-windows = naersk-lib.buildPackage rec {
            inherit LIBCLANG_PATH;
            name = "niketsu";
            version = VERSION;
            root = ./.;
            cargoBuildOptions = x: x ++ [ "--target" windows-target "--package" name ];
            cargoTestOptions = x: x ++ [ "--target" windows-target "--package" name ];
            buildInputs = with pkgs.pkgsCross.mingwW64.windows; [
              mingw_w64_pthreads
              pthreads
              pkgs.zip
            ];
            nativeBuildInputs = with pkgs;
              [ pkgsCross.mingwW64.buildPackages.gcc ];
            preBuild = ''
              export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="-C link-args=$(echo $NIX_LDFLAGS | tr ' ' '\n' | grep -- '^-L' | tr '\n' ' ')"
              export NIX_LDFLAGS=
            '';
            TARGET_CC = "${pkgs.pkgsCross.mingwW64.stdenv.cc}/bin/${pkgs.pkgsCross.mingwW64.stdenv.cc.targetPrefix}cc";
            BINDGEN_EXTRA_CLANG_ARGS = BINDGEN_EXTRA_LANG_ARGS pkgs.pkgsCross.mingwW64;
            MPV_SOURCE = WINDOWS_MPV_SOURCE;
            C_INCLUDE_PATH = "$C_INCLUDE_PATH:${WINDOWS_MPV_SOURCE}/include";
            postInstall = ''
              zip --junk-paths niketsu.zip $out/bin/niketsu.exe ${WINDOWS_MPV_SOURCE}/*dll*
              rm -dr $out
              mv niketsu.zip $out
            '';
          };
          niketsu-relay = naersk-lib.buildPackage rec {
            name = "niketsu-relay";
            version = VERSION;
            root = ./.;
            cargoBuildOptions = x: x ++ [ "--target" static-rust-target "--package" name ];
            cargoTestOptions = x: x ++ [ "--target" static-rust-target "--package" name ];
          };
        };
        devShells.default = (pkgs.devshell.mkShell {
          imports = [ "${devshell}/extra/git/hooks.nix" ];
          name = "niketsu-dev-shell";
          packages = with pkgs; [
            openssl
            rust-toolchain
            rust-analyzer
            cargo-audit
            cargo-outdated
            cargo-udeps
            cargo-nextest
            cargo-tarpaulin
            cargo-watch
            nixpkgs-fmt
            libclang
            gcc
            mdbook
            pkg-config
            yt-dlp
          ];
          env = [
            {
              name = "LD_LIBRARY_PATH";
              value = LD_LIBRARY_PATH;
            }
            {
              name = "LIBCLANG_PATH";
              value = LIBCLANG_PATH;
            }
            {
              name = "LIBRARY_PATH";
              value = LIBRARY_PATH;
            }
            {
              name = "C_INCLUDE_PATH";
              value = C_INCLUDE_PATH;
            }
            {
              name = "PKG_CONFIG_PATH";
              value = PKG_CONFIG_PATH;
            }
            {
              name = "PKG_CONFIG_SYSROOT_DIR";
              value = "/";
            }
          ];
          commands = [
            { package = "treefmt"; }
            {
              name = "udeps";
              category = "rust";
              command = ''
                PATH=${fenix.packages.${system}.latest.rustc}/bin:$PATH
                cargo udeps $@
              '';
              help = pkgs.cargo-udeps.meta.description;
            }
            {
              name = "outdated";
              category = "rust";
              command = "cargo-outdated outdated";
              help = pkgs.cargo-outdated.meta.description;
            }
            {
              name = "audit";
              category = "rust";
              command = "cargo-audit audit";
              help = pkgs.cargo-audit.meta.description;
            }
            {
              name = "nextest";
              category = "rust";
              command = "cargo-nextest nextest run";
              help = pkgs.cargo-nextest.meta.description;
            }
            {
              name = "tarpaulin";
              category = "rust";
              command = ''
                PATH=${fenix.packages.${system}.latest.rustc}/bin:$PATH
                cargo tarpaulin $@
              '';
              help = pkgs.cargo-tarpaulin.meta.description;
            }
          ];
        });
        checks = {
          nixpkgs-fmt = pkgs.runCommand "nixpkgs-fmt"
            {
              nativeBuildInputs = [ pkgs.nixpkgs-fmt ];
            } "nixpkgs-fmt --check ${./.}; touch $out";
          cargo-fmt = pkgs.runCommand "cargo-fmt"
            {
              nativeBuildInputs = [ rust-toolchain ];
            } "cd ${./.}; cargo fmt --check; touch $out";
          clippy = naersk-lib.buildPackage {
            inherit LIBCLANG_PATH;
            src = ./.;
            nativeBuildInputs = with pkgs; [ cmake pkg-config ] ++ libraries;
            mode = "clippy";
            preConfigure = ''
              export BINDGEN_EXTRA_CLANG_ARGS='${BINDGEN_EXTRA_LANG_ARGS pkgs}'
              export C_INCLUDE_PATH=$C_INCLUDE_PATH:${pkgs.mpv}/include
            '';
          };
        };
      });
}


