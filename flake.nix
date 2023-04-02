{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    utils.url = "github:numtide/flake-utils";
    devshell.url = "github:numtide/devshell";
    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, devshell, utils, fenix, ... }@inputs:
    utils.lib.eachSystem [ "aarch64-linux" "i686-linux" "x86_64-linux" ]
      (system:
        let
          lib = nixpkgs.lib;
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ devshell.overlays.default ];
          };
          rust-toolchain = with fenix.packages.${system};
            combine [
              stable.rustc
              stable.cargo
              stable.clippy
              latest.rustfmt
              targets.x86_64-unknown-linux-musl.stable.rust-std
            ];
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
          LIBRARY_PATH = lib.makeLibraryPath (with pkgs; [
            mpv
            xorg.libX11
            xorg.libXcursor
            xorg.libXi
            xorg.libXrandr
            expat
            freetype
            fontconfig
            vulkan-loader
          ]);
          PKG_CONFIG_PATH = lib.makeSearchPathOutput "dev" "lib/pkgconfig" (with pkgs; [ expat fontconfig freetype ]);
        in
        rec {
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
            ];
            git.hooks = {
              enable = true;
              pre-commit.text = "nix flake check";
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

