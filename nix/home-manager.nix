{ self }:
{ config, lib, pkgs, ... }:
let
  cfg = config.services.mycelium-node;
  launcher = import ./launcher.nix {
    inherit pkgs cfg;
    defaultConfigPath = "$HOME/.config/mycelium/node.toml";
  };
in
{
  options.services.mycelium-node = import ./options.nix {
    inherit lib;
    package = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    (lib.mkIf pkgs.stdenv.hostPlatform.isLinux {
      systemd.user.services.mycelium-node = {
        Unit = {
          Description = "Mycelium node";
          After = [ "network-online.target" ];
        };
        Service = {
          ExecStart = "${launcher}";
          Restart = "always";
          RestartSec = 3;
          KillMode = "process";
          TimeoutStartSec = "infinity";
          Environment = [ "PATH=${pkgs.git}/bin:/run/current-system/sw/bin:/usr/bin:/bin" ];
        };
        Install.WantedBy = [ "default.target" ];
      };
    })
    (lib.mkIf pkgs.stdenv.hostPlatform.isDarwin {
      launchd.agents.mycelium-node = {
        enable = true;
        config = {
          ProgramArguments = [ "${launcher}" ];
          RunAtLoad = true;
          KeepAlive = true;
          EnvironmentVariables.PATH = "${pkgs.git}/bin:/usr/local/bin:/usr/bin:/bin";
        };
      };
    })
  ]);
}
