{ ... }:
{
  boot.kernel.sysctl = {
    "net.core.rmem_max" = 26214400; # 25MB
    "net.core.rmem_default" = 26214400;
    "net.core.wmem_max" = 26214400;
    "net.core.wmem_default" = 26214400;
  };

  services.nginx = {
    enable = true;
    virtualHosts."stream.autumnal.de" = {
      enableACME = true;
      forceSSL = true;

      locations."/" = {
        proxyPass = "http://127.0.0.1:8889";
        proxyWebsockets = true;
        extraConfig = ''
          proxy_read_timeout 86400;
          proxy_send_timeout 86400;
          proxy_buffering off;
          tcp_nodelay on;

          proxy_set_header Host $host;
          proxy_set_header X-Real-IP $remote_addr;
          proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
          proxy_set_header X-Forwarded-Proto $scheme;
        '';
      };
    };
  };

  services.mediamtx = {
    enable = true;
    allowVideoAccess = false;

    settings = {
      authMethod = "internal";
      authInternalUsers = [
        {
          user = "any";
          pass = "";
          permissions = [
            {
              action = "read";
              path = "";
            }
            {
              action = "playback";
              path = "";
            }
          ];
        }
        {
          user = "default";
          pass = "sha256:rEIQz+qRia/cJR+A5WFIYI/i4ZOotrm+XOaQkiGTDOU=";
          permissions = [
            {
              action = "read";
              path = "";
            }
            {
              action = "playback";
              path = "";
            }
            {
              action = "publish";
              path = "";
            }
          ];
        }
      ];

      api = true;
      webrtc = true;
      webrtcAddress = ":8889";
      webrtcLocalUDPAddress = ":8189";
      webrtcTrustedProxies = [ "127.0.0.1" ];
      webrtcAdditionalHosts = [ "stream.autumnal.de" ];
      udpMaxPayloadSize = 1200;

      paths = {
        all_others = {
          source = "publisher";
        };
      };

      logLevel = "info";
      logDestinations = [ "stdout" ];
    };
  };

  networking.firewall.allowedUDPPorts = [ 8189 ];
  networking.firewall.allowedTCPPorts = [ 8554 ];
}
