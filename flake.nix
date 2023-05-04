{
  description = "Niketsu Server Configuration";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

    agenix = {
      url = "github:ryantm/agenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    deploy-rs = {
      url = "github:serokell/deploy-rs";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    niketsu.url = "github:sevenautumns/niketsu";

    devshell.url = "github:numtide/devshell";
  };

  outputs =
    { self, deploy-rs, nixpkgs, agenix, devshell, ... }@inputs:
    let lib = nixpkgs.lib;
    in {
      nixosConfigurations.niketsu = lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          {
            networking.hostName = "niketsu";
            nixpkgs.overlays = [ deploy-rs.overlay ];
          }
          agenix.nixosModules.default
          ./server.nix
        ];
        specialArgs = { inherit inputs; };
      };

      deploy.nodes.niketsu = {
        hostname = "autumnal.de";
        fastConnection = false;
        profiles.system = {
          sshUser = "admin";
          path = deploy-rs.lib.x86_64-linux.activate.nixos
            self.nixosConfigurations.niketsu;
          user = "root";
        };
      };

      devShells.x86_64-linux.default =
        let
          pkgs = import nixpkgs {
            system = "x86_64-linux";
            overlays = [ devshell.overlays.default deploy-rs.overlay ];
          };
        in
        (pkgs.devshell.mkShell {
          name = "niketsu-deploy-shell";
          packages = [
            pkgs.deploy-rs.deploy-rs
            pkgs.openssh
          ];
        });

      checks = builtins.mapAttrs
        (system: deployLib: deployLib.deployChecks self.deploy)
        deploy-rs.lib;
    };
}
