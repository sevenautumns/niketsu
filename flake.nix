{
  description = "Niketsu Server Configuration";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-24.05";

    agenix = {
      url = "github:ryantm/agenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    niketsu-stable.url = "github:sevenautumns/niketsu";
    niketsu-nightly.url = "github:sevenautumns/niketsu";

    devshell.url = "github:numtide/devshell";
  };

  outputs = { nixpkgs, agenix, devshell, ... }@inputs:
    let
      lib = nixpkgs.lib;
      pkgs = import nixpkgs {
        system = "x86_64-linux";
        overlays = [ devshell.overlays.default ];
      };
    in
    {
      nixosConfigurations.niketsu = lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          { networking.hostName = "niketsu"; }
          agenix.nixosModules.default
          ./server.nix
        ];
        specialArgs = { inherit inputs; };
      };

      devShells.x86_64-linux.default = (pkgs.devshell.mkShell {
        name = "niketsu-deploy-shell";
        packages = [ pkgs.openssh pkgs.nixos-rebuild ];
        commands = [{
          name = "deploy-niketsu";
          category = "deploy";
          command =
            let
              known-hosts = pkgs.writeText "known_hosts" ''
                autumnal.de ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOB5kFkv5hNVA0nbeIo1LtGZDOORTH+lXrxq8h2EmI3e
              '';
            in
            ''
              NIX_SSHOPTS="-o UserKnownHostsFile=${known-hosts}"
              export NIX_SSHOPTS
              nixos-rebuild \
                --flake .#niketsu \
                --use-remote-sudo \
                --target-host admin@autumnal.de \
                --build-host admin@autumnal.de \
                --use-substitutes \
                switch 
            '';
          help = "Deploy new system config";
        }];
      });
    };
}
