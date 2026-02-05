{
  pkgs,
  ...
}:

let
  portalPkg = pkgs.callPackage mediamtx_portal/default.nix { };
in
{
  systemd.services.autumnal-portal = {
    description = "MediaMTX Web Portal Service";
    after = [ "network.target" ];
    wantedBy = [ "multi-user.target" ];

    serviceConfig = {
      WorkingDirectory = "${portalPkg}/bin";

      ExecStart = "${portalPkg}/bin/portal";

      Restart = "always";
      RestartSec = "5s";

      DynamicUser = true;

      ProtectSystem = "full";
      NoNewPrivileges = true;
      PrivateTmp = true;
    };
  };

  services.nginx.virtualHosts."stream.autumnal.de" = {
    locations."/watch/" = {
      proxyPass = "http://127.0.0.1:8080/";
      extraConfig = ''
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
      '';
    };
  };
}
