{ lib, config, pkgs, inputs, ... }:
let
  system = pkgs.system;
  niketsu-stable = inputs.niketsu-stable.packages.${system}.niketsu-relay;
  niketsu-nightly = inputs.niketsu-nightly.packages.${system}.niketsu-relay;
  stable = 7766;
  nightly = 7777;
  ports = [ nightly ] ++ [ stable ];
  isNightly = p: (builtins.elem p [ nightly ]);
in
{
  # Template service for server
  systemd.services = builtins.listToAttrs (builtins.map
    (port: {
      name = "niketsu-${builtins.toString port}";
      value = let niketsu = if (isNightly port) then niketsu-nightly else niketsu-stable; in
        {
          enable = true;
          wantedBy = [ "multi-user.target" ];
          serviceConfig =
            {
              ExecStart = "${lib.meta.getExe' niketsu "niketsu-relay"} -t trace --port ${builtins.toString port}";
              User = config.users.users.niketsu.name;
              Restart = "always";
              RestartSec = 2;
              WorkingDirectory = "/var/lib/niketsu";
            };
        };
    })
    ports);

  # open all ports we use
  networking.firewall.allowedTCPPorts = ports;
  networking.firewall.allowedUDPPorts = ports;

  # create working directorie
  systemd.tmpfiles.rules = [
    "d '/var/lib/niketsu' 0700 ${config.users.users.niketsu.name} ${config.users.users.niketsu.group} - -"
    "d '/var/lib/niketsu/.config' 0700 ${config.users.users.niketsu.name} ${config.users.users.niketsu.group} - -"
  ];

  users.users.niketsu = {
    uid = 1025;
    isSystemUser = true;
    group = "niketsu";
    home = "/var/lib/niketsu";
    description = "Niketsu Service User";
  };
  users.groups.niketsu.gid = 992;
}
