{ hostname, domain }:

{ config, lib, pkgs, modulesPath, pikaServerPkg, sops-nix, ... }:

let
  serverPort = 8080;
  dbName = "pika_server";
in
{
  imports = [
    (modulesPath + "/profiles/qemu-guest.nix")
    ../modules/base.nix
  ];

  networking.hostName = hostname;

  services.openssh.openFirewall = lib.mkForce true;

  services.postgresql = {
    enable = true;
    ensureDatabases = [ dbName ];
    ensureUsers = [{
      name = dbName;
      ensureDBOwnership = true;
    }];
    authentication = lib.mkForce ''
      local all all trust
      host all all 127.0.0.1/32 trust
      host all all ::1/128 trust
    '';
  };

  services.caddy = {
    enable = true;
    virtualHosts.${domain} = {
      extraConfig = ''
        handle /health-check {
          reverse_proxy 127.0.0.1:${toString serverPort}
        }
        handle {
          reverse_proxy 127.0.0.1:${toString serverPort}
        }
      '';
    };
  };

  sops = {
    age.keyFile = "/etc/age/key.txt";
    defaultSopsFile = ../../secrets/pika-server.yaml;
  };

  sops.secrets."apns_key" = {
    format = "yaml";
    owner = "pika-server";
    group = "users";
    mode = "0400";
    path = "/var/lib/pika-server/apns-key.p8";
  };

  sops.secrets."apns_key_id" = {
    format = "yaml";
    owner = "pika-server";
    group = "users";
    mode = "0400";
  };

  sops.secrets."apns_team_id" = {
    format = "yaml";
    owner = "pika-server";
    group = "users";
    mode = "0400";
  };

  sops.secrets."fcm_credentials" = {
    format = "yaml";
    owner = "pika-server";
    group = "users";
    mode = "0400";
    path = "/var/lib/pika-server/fcm-credentials.json";
  };

  sops.templates."pika-server-env" = {
    owner = "pika-server";
    group = "users";
    mode = "0400";
    content = ''
      DATABASE_URL=postgresql://${dbName}@/${dbName}
      RELAYS=wss://us-east.nostr.pikachat.org,wss://eu.nostr.pikachat.org,wss://relay.primal.net,wss://nos.lol,wss://relay.damus.io
      NOTIFICATION_PORT=${toString serverPort}
      APNS_KEY_PATH=${config.sops.secrets."apns_key".path}
      APNS_KEY_ID=${config.sops.placeholder."apns_key_id"}
      APNS_TEAM_ID=${config.sops.placeholder."apns_team_id"}
      APNS_TOPIC=org.pikachat.pika
      # FCM_CREDENTIALS_PATH=${config.sops.secrets."fcm_credentials".path}
      RUST_LOG=info
    '';
  };

  users.users."pika-server" = {
    isSystemUser = true;
    group = "pika-server";
    home = "/var/lib/pika-server";
    createHome = true;
  };
  users.groups."pika-server" = {};

  systemd.services.pika-server = {
    description = "Pika notification server";
    wantedBy = [ "multi-user.target" ];
    after = [ "network-online.target" "postgresql.service" "sops-nix.service" ];
    wants = [ "network-online.target" ];
    requires = [ "postgresql.service" ];

    restartTriggers = [
      config.sops.templates."pika-server-env".path
      pikaServerPkg
    ];

    serviceConfig = {
      Type = "simple";
      User = "pika-server";
      Group = "pika-server";
      WorkingDirectory = "/var/lib/pika-server";
      EnvironmentFile = [ config.sops.templates."pika-server-env".path ];
      ExecStart = "${pikaServerPkg}/bin/pika-server";
      Restart = "always";
      RestartSec = "2s";
      NoNewPrivileges = true;
      PrivateTmp = true;
      ProtectSystem = "strict";
      ProtectHome = true;
      ReadWritePaths = [ "/var/lib/pika-server" ];
    };
  };

  systemd.tmpfiles.rules = [
    "d /var/lib/pika-server 0750 pika-server pika-server -"
    "d /etc/age 0700 root root -"
  ];

  networking.firewall = {
    allowedTCPPorts = [ 80 443 ];
  };

  disko.devices = {
    disk.main = {
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
  };

  boot.loader.grub = {
    enable = true;
    efiSupport = true;
    efiInstallAsRemovable = true;
  };

  environment.systemPackages = with pkgs; [
    (writeShellScriptBin "pika-server-status" ''
      echo "=== pika-server status ==="
      systemctl status pika-server --no-pager
      echo ""
      echo "=== Recent logs ==="
      journalctl -u pika-server -n 30 --no-pager
      echo ""
      echo "=== PostgreSQL ==="
      systemctl status postgresql --no-pager -n 5
    '')
    (writeShellScriptBin "pika-server-logs" ''
      journalctl -u pika-server -f
    '')
    (writeShellScriptBin "pika-server-restart" ''
      systemctl restart pika-server
      sleep 2
      systemctl is-active pika-server && echo "Service is running" || echo "Service failed to start"
    '')
  ];

  system.stateVersion = "24.05";
}
