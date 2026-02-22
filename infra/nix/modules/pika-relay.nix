{ hostname, domain }:

{ config, lib, pkgs, modulesPath, pikaRelayPkg, ... }:

let
  dataDir = "/var/lib/pika-relay";
  mediaDir = "${dataDir}/media";
  serviceURL = "https://${domain}";
in
{
  imports = [
    (modulesPath + "/profiles/qemu-guest.nix")
    ./base.nix
  ];

  networking.hostName = hostname;

  # Caddy: TLS termination + reverse proxy
  services.caddy = {
    enable = true;
    virtualHosts."${domain}" = {
      extraConfig = ''
        reverse_proxy localhost:3334
      '';
    };
  };

  # pika-relay service
  systemd.services.pika-relay = {
    description = "Pika relay + Blossom media server";
    wantedBy = [ "multi-user.target" ];
    after = [ "network-online.target" ];
    wants = [ "network-online.target" ];

    environment = {
      PORT = "3334";
      DATA_DIR = dataDir;
      MEDIA_DIR = mediaDir;
      SERVICE_URL = serviceURL;
      RELAY_NAME = "pika-relay (${hostname})";
      RELAY_DESCRIPTION = "Pika relay + Blossom media server";
    };

    serviceConfig = {
      ExecStart = "${pikaRelayPkg}/bin/pika-relay";
      Restart = "always";
      RestartSec = 5;

      DynamicUser = true;
      StateDirectory = "pika-relay";
      StateDirectoryMode = "0750";

      NoNewPrivileges = true;
      PrivateTmp = true;
      ProtectSystem = "strict";
      ProtectHome = true;
      ReadWritePaths = [ dataDir ];
    };
  };

  services.openssh.openFirewall = lib.mkForce true;

  # Firewall: HTTP(S) for Caddy + SSH
  networking.firewall.allowedTCPPorts = [ 80 443 ];

  # Disk layout
  disko.devices.disk.main = {
    type = "disk";
    device = "/dev/sda";
    content = {
      type = "gpt";
      partitions = {
        boot = {
          size = "1M";
          type = "EF02";
        };
        esp = {
          size = "512M";
          type = "EF00";
          content = {
            type = "filesystem";
            format = "vfat";
            mountpoint = "/boot";
          };
        };
        root = {
          size = "100%";
          content = {
            type = "filesystem";
            format = "ext4";
            mountpoint = "/";
          };
        };
      };
    };
  };

  boot.loader.grub = {
    enable = true;
    efiSupport = true;
    efiInstallAsRemovable = true;
  };

  system.stateVersion = "24.11";

  # Helper scripts
  environment.systemPackages = [
    (pkgs.writeShellScriptBin "pika-relay-status" "systemctl status pika-relay caddy")
    (pkgs.writeShellScriptBin "pika-relay-logs" "journalctl -u pika-relay -f")
    (pkgs.writeShellScriptBin "pika-relay-restart" "systemctl restart pika-relay")
  ];
}
