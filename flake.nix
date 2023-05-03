{
  description = "Niketsu Server Configuration";

  inputs = {
    nixpkgs-stable.url = "github:nixos/nixpkgs/nixos-22.11";
    nixpkgs-unstable.url = "github:nixos/nixpkgs/nixos-unstable";

    agenix = {
      url = "github:ryantm/agenix";
      inputs.nixpkgs.follows = "nixpkgs-unstable";
    };

    deploy-rs = {
      url = "github:serokell/deploy-rs";
      inputs.nixpkgs.follows = "nixpkgs-unstable";
    };

    niketsu.url = "github:sevenautumns/niketsu";
  };

  outputs =
    { self, deploy-rs, nixpkgs-unstable, nixpkgs-stable, agenix, ... }@inputs:
    let lib = nixpkgs-unstable.lib;
    in {
      nixosConfigurations.niketsu = lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          {
            networking.hostName = "niketsu";
            nixpkgs.overlays =
              [ deploy-rs.overlay self.overlays.matryoshka-pkgs ];
          }
          agenix.nixosModules.default
          ./server.nix
        ];
        specialArgs = { inherit inputs; };
      };

      # Overlay for always having stable and unstable accessible
      overlays.matryoshka-pkgs = final: prev: {
        unstable = import "${inputs.nixpkgs-unstable}" {
          system = prev.system;
          config.allowUnfree = true;
        };
        stable = import "${inputs.nixpkgs-stable}" {
          system = prev.system;
          config.allowUnfree = true;
        };
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

      checks = builtins.mapAttrs
        (system: deployLib: deployLib.deployChecks self.deploy) deploy-rs.lib;
    };
}
