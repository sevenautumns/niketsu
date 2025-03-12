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

  outputs =
    {
      nixpkgs,
      devshell,
      utils,
      fenix,
      naersk,
      ...
    }:
    utils.lib.eachSystem [ "aarch64-linux" "x86_64-linux" ] (
      system:
      let
        lib = nixpkgs.lib;
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ devshell.overlays.default ];
        };
        static-rust-target = pkgs.pkgsStatic.targetPlatform.rust.rustcTarget;
        windows-rust = pkgs.pkgsCross.mingwW64.targetPlatform.rust;
        rust-toolchain =
          with fenix.packages.${system};
          combine [
            stable.rustc
            stable.cargo
            stable.clippy
            stable.rust-analyzer
            stable.rust-src
            latest.rustfmt
            targets.${windows-rust.rustcTarget}.stable.rust-std
            targets.${static-rust-target}.stable.rust-std
          ];
        naersk-lib = (
          naersk.lib.${system}.override {
            cargo = rust-toolchain;
            rustc = rust-toolchain;
          }
        );
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
        VERSION = (with builtins; (fromTOML (readFile ./client/Cargo.toml))).package.version;
        WINDOWS_MPV_SOURCE = pkgs.stdenv.mkDerivation {
          name = "mpv-windows";
          src = pkgs.fetchurl {
            url = "https://phoenixnap.dl.sourceforge.net/project/mpv-player-windows/libmpv/mpv-dev-x86_64-v3-20250309-git-edf4fdf.7z";
            sha256 = "sha256-eDfujbmKD9AQqL22kRExOq6NWfK+/EWLMQG3zryQh+M=";
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
            name = "niketsu";
            version = VERSION;
            root = ./.;
            cargoBuildOptions = lib.concat [ "-p ${name}" ];
            cargoTestOptions = lib.concat [ "-p ${name}" ];
            nativeBuildInputs = with pkgs; [
              cmake
              pkg-config
              rustPlatform.bindgenHook
            ];
            buildInputs = with pkgs; [ yt-dlp ] ++ libraries;
            C_INCLUDE_PATH = "$C_INCLUDE_PATH:${pkgs.mpv}/include";
          };
          niketsu-client-windows = naersk-lib.buildPackage rec {
            name = "niketsu";
            version = VERSION;
            root = ./.;
            CARGO_BUILD_TARGET = windows-rust.rustcTarget;
            cargoBuildOptions = lib.concat [ "-p ${name}" ];
            cargoTestOptions = lib.concat [ "-p ${name}" ];
            buildInputs = with pkgs.pkgsCross.mingwW64.windows; [
              mingw_w64_pthreads
              pthreads
              pkgs.zip
            ];
            nativeBuildInputs = with pkgs; [
              pkgsCross.mingwW64.buildPackages.gcc
              pkgsCross.mingwW64.pkgsBuildHost.rustPlatform.bindgenHook
            ];
            preBuild = ''
              export CARGO_TARGET_${windows-rust.cargoEnvVarTarget}_RUSTFLAGS="-C link-args=$(echo $NIX_LDFLAGS | tr ' ' '\n' | grep -- '^-L' | tr '\n' ' ')"
              export NIX_LDFLAGS=
            '';
            TARGET_CC = "${pkgs.pkgsCross.mingwW64.stdenv.cc}/bin/${pkgs.pkgsCross.mingwW64.stdenv.cc.targetPrefix}cc";
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
            cargoBuildOptions = lib.concat [ "-p ${name}" ];
            cargoTestOptions = lib.concat [ "-p ${name}" ];
            root = ./.;
            CARGO_BUILD_TARGET = static-rust-target;
          };
        };
        devShells.default = pkgs.mkShell {
          inputsFrom = [ packages.niketsu-client ];
          nativeBuildInputs = with pkgs; [
            rust-toolchain
            cargo-audit
            cargo-outdated
            cargo-udeps
            cargo-nextest
            cargo-tarpaulin
            cargo-watch
            mdbook
            yt-dlp
          ];
        };
        checks = {
          nixpkgs-fmt = pkgs.runCommand "nixpkgs-fmt" {
            nativeBuildInputs = [ pkgs.nixpkgs-fmt ];
          } "nixpkgs-fmt --check ${./.}; touch $out";
          cargo-fmt = pkgs.runCommand "cargo-fmt" {
            nativeBuildInputs = [ rust-toolchain ];
          } "cd ${./.}; cargo fmt --check; touch $out";
          cargo-clippy = naersk-lib.buildPackage {
            src = ./.;
            nativeBuildInputs = with pkgs; [ rustPlatform.bindgenHook ];
            mode = "clippy";
          };
        };
      }
    );
}
