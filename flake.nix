{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    utils.url = "github:numtide/flake-utils";
    devshell.url = "github:numtide/devshell";
    fenix.url = "github:nix-community/fenix";
    naersk.url = "github:nix-community/naersk";
  };

  outputs = { self, nixpkgs, devshell, utils, fenix, naersk, ... }@inputs:
    utils.lib.eachSystem [ "aarch64-linux" "x86_64-linux" ] (system:
      let
        lib = nixpkgs.lib;
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ devshell.overlays.default ];
        };
        host-target = pkgs.rust.toRustTargetSpec pkgs.stdenv.hostPlatform;
        rust-toolchain = with fenix.packages.${system};
          combine [
            stable.rustc
            stable.cargo
            stable.clippy
            latest.rustfmt
            targets.x86_64-pc-windows-gnu.stable.rust-std
          ];
        naersk-lib = (naersk.lib.${system}.override {
          cargo = rust-toolchain;
          rustc = rust-toolchain;
        });
        C_INCLUDE_PATH = lib.makeSearchPathOutput "dev" "include"
          (with pkgs; [ xorg.libX11 mpv-unwrapped fontconfig freetype expat musl ]);
        LD_LIBRARY_PATH = lib.makeLibraryPath [ pkgs.libclang.lib ];
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
        MPV_SOURCE = pkgs.stdenv.mkDerivation {
          name = "mpv-windows";
          src = pkgs.fetchurl {
            url =
              "https://altushost-swe.dl.sourceforge.net/project/mpv-player-windows/libmpv/mpv-dev-x86_64-v3-20230423-git-c7a8e71.7z";
            sha256 = "sha256-/BLNQZDGpSPJP3DfkjDBBh/FM1OEFMZxPyIjdb6cHPM=";
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
            nativeBuildInputs = with pkgs; [ cmake pkgconfig ] ++ libraries;
            buildInputs = with pkgs; [ yt-dlp ];
            LIBCLANG_PATH =
              lib.makeLibraryPath [ pkgs.llvmPackages.libclang.lib ];
            preConfigure = ''
              export BINDGEN_EXTRA_CLANG_ARGS='${BINDGEN_EXTRA_LANG_ARGS pkgs}'
              export C_INCLUDE_PATH=$C_INCLUDE_PATH:${pkgs.mpv}/include
            '';
          };
          niketsu-client-windows = naersk-lib.buildPackage rec {
            name = "niketsu";
            version = VERSION;
            root = ./.;
            cargoBuildOptions = x: x ++ [ "--target" "x86_64-pc-windows-gnu" ];
            cargoTestOptions = x: x ++ [ "--target" "x86_64-pc-windows-gnu" ];
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
            LIBCLANG_PATH =
              lib.makeLibraryPath [ pkgs.llvmPackages.libclang.lib ];
            preConfigure = ''
              export BINDGEN_EXTRA_CLANG_ARGS='${
                BINDGEN_EXTRA_LANG_ARGS pkgs.pkgsCross.mingwW64
              }'
              export C_INCLUDE_PATH=$C_INCLUDE_PATH:${MPV_SOURCE}/include
              export MPV_SOURCE=${MPV_SOURCE}
            '';
            postInstall = ''
              zip --junk-paths niketsu.zip $out/bin/niketsu.exe ${MPV_SOURCE}/*dll*
              rm -dr $out
              mv niketsu.zip $out
            '';
          };
          niketsu-server = pkgs.buildGoModule rec {
            name = "niketsu-server";
            version = VERSION;
            src = ./.;
            buildInputs = with pkgs; [ stdenv go glibc.static ];
            ldflags =
              [ "-s" "-w" "-linkmode external" "-extldflags" "-static" ];
            postInstall = ''
              mv $out/bin/server $out/bin/niketsu-server
            '';
            vendorHash = "sha256-HOVTtj32nqZGTYQMQdH5gDihPLM0FSB5MyRJcEfz24w=";
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
            mdbook
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

