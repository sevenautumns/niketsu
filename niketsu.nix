{ config, lib, pkgs, inputs, ... }:
let
  system = pkgs.system;
  niketsu = inputs.niketsu.packages.${system}.niketsu-server;
  server = {
    stable = {
      secure = lib.range 7766 7778;
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
    (builtins.elem p (with server; stable.secure ++ nightly.insecure));
  isNightly = p: (builtins.elem p nightlyPorts);
in
{
  # Template service for server
  systemd.services = {
    "niketsu@" = {
      enable = true;
      serviceConfig = {
        User = config.users.users.niketsu.name;
        ExecStart = "niketsu-server";
        Restart = "always";
        RestartSec = 2;
        WorkingDirectory = "/var/lib/niketsu-%i";
      };
    };
  } // builtins.listToAttrs (builtins.map
    (p: {
      name = "niketsu@${builtins.toString p}";
      value = {
        wantedBy = [ "multi-user.target" ];
        overrideStrategy = "asDropin";
        environment = lib.attrsets.optionalAttrs (isSecure p) {
          CERT = "${config.security.acme.certs."autumnal.de".directory}/cert.pem";
          KEY = "${config.security.acme.certs."autumnal.de".directory}/key.pem";
        };
        path =
          if (isNightly p) then
            [ inputs.niketsu-nightly.packages.${system}.niketsu-server ]
          else
            [ inputs.niketsu-stable.packages.${system}.niketsu-server ];
      };
    })
    ports);

  # open all ports we use
  networking.firewall.allowedTCPPorts = ports;

  # create working directories
  systemd.tmpfiles.rules = builtins.map
    (p:
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
  users.groups.niketsu = {};
}
