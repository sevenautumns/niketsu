{ config, ... }:
let
  root = "autumnal.de";
  rootCertDir = config.security.acme.certs."${root}".directory;
in
{
  users.groups.root_cert.members = [
    "murmur"
    "nginx"
  ];

  security.acme.certs."autumnal.de" = {
    group = "root_cert";
    reloadServices = [ "murmur" ];
  };

  services.murmur = {
    enable = true;
    textMsgLength = 0;
    openFirewall = true;
    bandwidth = 500000;
    sslCa = "${rootCertDir}/chain.pem";
    sslCert = "${rootCertDir}/fullchain.pem";
    sslKey = "${rootCertDir}/key.pem";
    extraConfig = ''
      allowRecording=false
    '';
  };
}
