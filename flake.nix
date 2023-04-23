{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    utils.url = "github:numtide/flake-utils";
    devshell.url = "github:numtide/devshell";
    fenix.url = "github:nix-community/fenix";
    naersk.url = "github:nix-community/naersk";
  };

  outputs = { self, nixpkgs, devshell, utils, fenix, naersk, ... }@inputs:
    utils.lib.eachSystem [ "aarch64-linux" "x86_64-linux" ]
      (system:
        let
          lib = nixpkgs.lib;
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ devshell.overlays.default ];
          };
          host-target = pkgs.rust.toRustTargetSpec pkgs.stdenv.hostPlatform;
          musl-target = pkgs.rust.toRustTargetSpec pkgs.pkgsMusl.stdenv.hostPlatform;
          rust-toolchain = with fenix.packages.${system};
            combine [
              stable.rustc
              stable.cargo
              stable.clippy
              latest.rustfmt
              targets.${musl-target}.stable.rust-std
            ];
          naersk-lib = (naersk.lib.${system}.override {
            cargo = rust-toolchain;
            rustc = rust-toolchain;
          });
          # C_INCLUDE_PATH = with pkgs;
          #   lib.concatStringsSep ":" [
          #     "${mpv}/include"
          #     "${lib.getDev fontconfig}"
          #     "${xorg.libX11.dev}/include"
          #   ];
          # LIBCLANG_PATH = pkgs.lib.makeLibraryPath [ pkgs.libclang.lib ];
          C_INCLUDE_PATH = lib.makeSearchPathOutput "dev" "include"
            (with pkgs; [ xorg.libX11 mpv fontconfig freetype expat musl ]);
          LD_LIBRARY_PATH = lib.makeLibraryPath [ pkgs.libclang.lib ];
          libraries = with pkgs; [
            mpv
            xorg.libX11
            xorg.libXcursor
            xorg.libXi
            xorg.libXrandr
            expat
            freetype
            fontconfig
            vulkan-loader
            wayland
            wayland-protocols
            libxkbcommon
          ];
          LIBRARY_PATH = lib.makeLibraryPath libraries;
          PKG_CONFIG_PATH = lib.makeSearchPathOutput "dev" "lib/pkgconfig"
            (with pkgs; [ expat fontconfig freetype ]);
        in
        rec {
          packages = {
            default = packages.niketsu-client;
            niketsu-client = naersk-lib.buildPackage rec {
              pname = "niketsu";
              root = ./.;
              cargoBuildOptions = x:
                x ++ [ "--target" musl-target ];
              cargoTestOptions = x:
                x ++ [ "--target" musl-target ];
              nativeBuildInputs = with pkgs; [ cmake pkgconfig ] ++ libraries;
              LIBCLANG_PATH =
                lib.makeLibraryPath [ pkgs.llvmPackages.libclang.lib ];
              preConfigure = ''
                export BINDGEN_EXTRA_CLANG_ARGS='-isystem ${
                  lib.makeSearchPathOutput "dev" "include" [ pkgs.musl ]
                }'
                export LD_LIBRARY_PATH=${LD_LIBRARY_PATH}
                export LIBRARY_PATH=${LIBRARY_PATH}
                export C_INCLUDE_PATH=${C_INCLUDE_PATH}
              '';
            };
            niketsu-server = pkgs.buildGoModule rec {
              pname = "niketsu-server";
              version = "0.1.0";
              src = ./.;
              buildInputs = with pkgs; [ stdenv go glibc.static ];
              ldflags = [
                "-s"
                "-w"
                "-linkmode external"
                "-extldflags"
                "-static"
              ];
              postInstall = ''
                mv $out/bin/server $out/bin/niketsu-server
              '';
              vendorHash = "sha256-JsGAq0ETM00yN60IbnN/uRF4dtu/MQ1Hxu5OjcD/MRg=";
            };
          };
          devShells.default = (pkgs.devshell.mkShell {
            imports = [ "${devshell}/extra/git/hooks.nix" ];
            name = "niketsu-dev-shell";
            packages = with pkgs; [
              rust-toolchain
              rust-analyzer
              cargo-outdated
              cargo-udeps
              cargo-watch
              nixpkgs-fmt
              libclang
              gcc
              musl.dev
              pkgconfig
              yt-dlp
              go
            ];
            git.hooks = {
              enable = true;
              # pre-commit.text = "nix flake check";
            };
            env = [
              {
                name = "LD_LIBRARY_PATH";
                value = LD_LIBRARY_PATH;
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
                command = ''
                  PATH=${fenix.packages.${system}.latest.rustc}/bin:$PATH
                  cargo udeps $@
                '';
                help = pkgs.cargo-udeps.meta.description;
              }
              {
                name = "outdated";
                command = "cargo-outdated outdated";
                help = pkgs.cargo-outdated.meta.description;
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
          };
        });
}

