{ ... }:
{

  services.galene = {
    enable = true;
    insecure = true;
    httpAddress = "0.0.0.0";
    turnAddress = "0.0.0.0:1194";
  };

  services.nginx = {
    virtualHosts."stream.autumnal.de" = {
      enableACME = true;
      forceSSL = true;

      locations."/" = {
        proxyPass = "http://127.0.0.1:8443";
        proxyWebsockets = true;
        extraConfig = ''
          client_max_body_size 0;

          proxy_set_header Host $host;
          proxy_set_header X-Real-IP $remote_addr;
          proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
          proxy_set_header X-Forwarded-Proto $scheme;

          add_header Content-Security-Policy "upgrade-insecure-requests";
        '';
      };
    };
  };

  networking.firewall.allowedUDPPorts = [ 1194 ];
}
