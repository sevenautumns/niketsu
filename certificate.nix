{ pkgs, ... }: {
  networking.firewall.allowedTCPPorts = [ 80 443 ];

  services.nginx = {
    enable = true;
    virtualHosts = {
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

