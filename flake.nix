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

    niketsu-stable.url = "github:sevenautumns/niketsu";
    niketsu-nightly.url = "github:sevenautumns/niketsu";

    devshell.url = "github:numtide/devshell";
  };

  outputs = { self, deploy-rs, nixpkgs, agenix, devshell, ... }@inputs:
    let
      lib = nixpkgs.lib;
      pkgs = import nixpkgs {
        system = "x86_64-linux";
        overlays = [ devshell.overlays.default deploy-rs.overlay ];
      };
    in
    {
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

      deploy.nodes.niketsu =
        let
          known-hosts = pkgs.writeText "known_hosts" ''
            autumnal.de ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOB5kFkv5hNVA0nbeIo1LtGZDOORTH+lXrxq8h2EmI3e
          '';
        in
        {
          hostname = "autumnal.de";
          fastConnection = false;
          sshOpts = [ "-o" "UserKnownHostsFile=${known-hosts}" ];
          profiles.system = {
            sshUser = "admin";
            path = deploy-rs.lib.x86_64-linux.activate.nixos
              self.nixosConfigurations.niketsu;
            user = "root";
          };
        };

      devShells.x86_64-linux.default = (pkgs.devshell.mkShell {
        name = "niketsu-deploy-shell";
        packages = [ pkgs.deploy-rs.deploy-rs pkgs.openssh ];
      });

      checks = builtins.mapAttrs
        (system: deployLib: deployLib.deployChecks self.deploy)
        deploy-rs.lib;
    };
}
