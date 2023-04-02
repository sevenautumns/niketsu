{
  inputs = {
    utils.url = "github:numtide/flake-utils";
    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, utils, fenix, ... }@inputs:
    utils.lib.eachSystem [ "aarch64-linux" "i686-linux" "x86_64-linux" ]
    (system:
      let
        pkgs = import nixpkgs { inherit system; };
        rust-toolchain = with fenix.packages.${system};
          combine [
            stable.rustc
            stable.cargo
            stable.clippy
            latest.rustfmt
            targets.x86_64-unknown-linux-musl.stable.rust-std
          ];
        C_INCLUDE_PATH = with pkgs;
          lib.concatStringsSep ":" [
            "${mpv}/include"
            "${lib.getDev fontconfig}"
          ];
        LIBCLANG_PATH = pkgs.lib.makeLibraryPath [ pkgs.libclang.lib ];
      in rec {
        devShells.default = (pkgs.mkShell {
          shellHook = ''
            export LIBCLANG_PATH=${LIBCLANG_PATH}
          '';
          buildInputs = with pkgs; [
            clang
            rust-toolchain
            rust-analyzer
            cargo-outdated
            cargo-udeps
            cargo-audit
            cargo-watch
            nixpkgs-fmt
          ];
          nativeBuildInputs = with pkgs; [
            pkg-config
            fontconfig
            mpv
            libclang.lib
            xorg.libX11
            xorg.libXcursor
            xorg.libXrandr
            xorg.libXi
          ];
        });
        checks = {
          nixpkgs-fmt = pkgs.runCommand "nixpkgs-fmt" {
            nativeBuildInputs = [ pkgs.nixpkgs-fmt ];
          } "nixpkgs-fmt --check ${./.}; touch $out";
          cargo-fmt = pkgs.runCommand "cargo-fmt" {
            nativeBuildInputs = [ rust-toolchain ];
          } "cd ${./.}; cargo fmt --check; touch $out";
        };
      });
}

