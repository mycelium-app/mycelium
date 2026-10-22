{ self }:
{ config, lib, pkgs, ... }:
let
  cfg = config.services.mycelium-node;
  launcher = import ./launcher.nix {
    inherit pkgs cfg;
    defaultConfigPath = "/var/lib/mycelium-node/node.toml";
  };
in
{
  options.services.mycelium-node = (import ./options.nix {
    inherit lib;
    package = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
  }) // {
    user = lib.mkOption {
      type = lib.types.str;
      default = "root";
      description = "User the node daemon and its agents run as.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.mycelium-node = {
      description = "Mycelium node";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      path = [ pkgs.git ];
      environment.MYCELIUM_NODE_CONFIG = "/var/lib/mycelium-node/node.toml";
      serviceConfig = {
        ExecStart = launcher;
        User = cfg.user;
        StateDirectory = "mycelium-node";
        Restart = "always";
        RestartSec = 3;
        KillMode = "process";
        TimeoutStartSec = "infinity";
      };
    };
  };
}
