{ config, lib, pkgs, inputs, ... }:
let
  system = pkgs.system;
  niketsu = inputs.niketsu.packages.${system}.niketsu-server;
in
{
  systemd.services = {
    "niketsu@" = {
      enable = true;
      serviceConfig = {
        User = "niketsu";
        ExecStart = "${niketsu}/bin/niketsu-server --config config.toml";
        Restart = "always";
        RestartSec = 2;
        WorkingDirectory = "/var/lib/niketsu-%i";
      };
    };
  } // builtins.listToAttrs (builtins.map
    (u: {
      name = "niketsu@${u}";
      value = {
        wantedBy = [ "multi-user.target" ];
        overrideStrategy = "asDropin";
      };
    }) [ "0" "1" "2" "3" "4" "5" ]);

  networking.firewall.allowedTCPPorts = [ 7766 7767 7768 7769 7770 7771 ];

  users.users.niketsu = {
    uid = 1025;
    isSystemUser = true;
    group = "nginx";
    description = "Niketsu Service User";
  };
}
