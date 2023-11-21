{ pkgs, ... }: {
  networking.firewall.allowedTCPPorts = [ 80 443 ];

  services.nginx = {
    enable = true;
    virtualHosts = {
      "niketsu.de" = {
        forceSSL = true;
        enableACME = true;
        locations = {
          "/" = {
            proxyPass = "https://sevenautumns.github.io/niketsu";
            extraConfig = ''
              proxy_set_header Host sevenautumns.github.io/niketsu;
            '';
          };
        };
      };
      "autumnal.de" = {
        forceSSL = true;
        enableACME = true;
      };
    };
  };

  security.acme = {
    acceptTerms = true;
    defaults.email = "sven@autumnal.de";
  };
}

