{ config, lib, pkgs, inputs, ... }:
let
  system = pkgs.system;
  niketsu-stable = inputs.niketsu-stable.packages.${system}.niketsu-server;
  niketsu-nightly = inputs.niketsu-nightly.packages.${system}.niketsu-server;
  server = {
    stable = {
      secure = lib.range 7766 7777;
      insecure = [ 3333 4444 ];
    };
    nightly = {
      secure = [ 6969 9696 42069 ];
      insecure = [ 2222 ];
    };
  };
  nightlyPorts = with server.nightly; secure ++ insecure;
  stablePorts = with server.stable; secure ++ insecure;
  ports = nightlyPorts ++ stablePorts;
  isSecure = p:
    (builtins.elem p (with server; stable.secure ++ nightly.secure));
  isNightly = p: (builtins.elem p nightlyPorts);
in {
  # Template service for server
  systemd.services = builtins.listToAttrs (builtins.map (p: {
    name = "niketsu-${builtins.toString p}";
    value = {
      enable = true;
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        ExecStart = "${
            if (isNightly p) then niketsu-nightly else niketsu-stable
          }/bin/niketsu-server";
        User = config.users.users.niketsu.name;
        Restart = "always";
        RestartSec = 2;
        WorkingDirectory = "/var/lib/niketsu-${builtins.toString p}";
      };
      environment = lib.attrsets.optionalAttrs (isSecure p) {
        CERT = "${config.security.acme.certs."autumnal.de".directory}/cert.pem";
        KEY = "${config.security.acme.certs."autumnal.de".directory}/key.pem";
        PORT = builtins.toString p;
      };
    };
  }) ports);

  # open all ports we use
  networking.firewall.allowedTCPPorts = ports;

  # create working directories
  systemd.tmpfiles.rules = builtins.map (p:
    "d '/var/lib/niketsu-${
      builtins.toString p
    }' 0700 ${config.users.users.niketsu.name} ${config.users.users.niketsu.group} - -")
    ports;

  users.users.niketsu = {
    uid = 1025;
    isSystemUser = true;
    group = "niketsu";
    # needs to be in the nginx group for access to the certificates
    extraGroups = [ "nginx" ];
    description = "Niketsu Service User";
  };
  users.groups.niketsu.gid = 992;
}
