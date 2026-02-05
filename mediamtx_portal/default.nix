{
  pkgs ? import <nixpkgs> { },
}:

pkgs.stdenv.mkDerivation {
  pname = "mediamtx-portal";
  version = "1.0.0";

  src = ./.;

  nativeBuildInputs = [ pkgs.go ];

  buildPhase = ''
    export GOCACHE=$TMPDIR/go-cache
    export GOPATH=$TMPDIR/go
    go build -o portal main.go
  '';

  installPhase = ''
    mkdir -p $out/bin
    cp portal $out/bin/
    cp index.html $out/bin/
  '';

  meta = with pkgs.lib; {
    description = "MediaMTX Web Portal";
    platforms = platforms.linux ++ platforms.darwin;
  };
}
