{ config, lib, modulesPath, ... }: {
  imports = [
    (modulesPath + "/profiles/qemu-guest.nix")
    ./niketsu.nix
    ./certificate.nix
  ];

  boot.loader.grub.enable = true;
  boot.loader.grub.device = "/dev/vda";
  boot.initrd.availableKernelModules =
    [ "ata_piix" "uhci_hcd" "virtio_pci" "sr_mod" "virtio_blk" ];
  boot.initrd.kernelModules = [ ];
  boot.kernelModules = [ ];
  boot.extraModulePackages = [ ];

  networking.useDHCP = false;
  networking.interfaces.ens3.useDHCP = true;
  networking.enableIPv6 = true;

  services.journald.extraConfig = "SystemMaxUse=250M";
  time.timeZone = "Europe/Berlin";

  # Select internationalisation properties.
  i18n.defaultLocale = "en_GB.UTF-8";

  services.openssh = {
    enable = true;
    settings.PasswordAuthentication = false;
  };

  nix.gc.automatic = true;

  networking.networkmanager.enable = true;

  nix = {
    settings.trusted-users = [ "admin" ];
    extraOptions = ''
      experimental-features = nix-command flakes
    '';
  };

  security.sudo.extraRules = [{
    users = [ "admin" ];
    commands = [{
      command = "ALL";
      options = [ "NOPASSWD" ];
    }];
  }];

  users.users.admin = {
    uid = 1001;
    isNormalUser = true;
    extraGroups = [ "wheel" "sudo" ];
    openssh.authorizedKeys.keys = [
      "sk-ssh-ed25519@openssh.com AAAAGnNrLXNzaC1lZDI1NTE5QG9wZW5zc2guY29tAAAAID6cRpwV5pivNp8GWF3uAw4yOEJIYGkfMchIUeL+3f3hAAAACXNzaDp5azUuMQ== ssh:yk5.1"
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDp9uGfZbpd/Xyk2ulzEsdCYJ6XsDHHSQbMSIb00LP/X niketsu@github.com"
    ];
  };

  # File systems configuration for using the installer's partition layout
  fileSystems = {
    "/" = {
      device = "/dev/disk/by-label/nixos";
      fsType = "btrfs";
    };
  };
  swapDevices = [{ device = "/dev/disk/by-label/swap"; }];

  hardware.cpu.amd.updateMicrocode =
    lib.mkDefault config.hardware.enableRedistributableFirmware;
  system.stateVersion = "22.11";
}
