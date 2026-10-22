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

  config = lib.mkIf cfg.enable {
    launchd.user.agents.mycelium-node.serviceConfig = {
      ProgramArguments = [ "${launcher}" ];
      RunAtLoad = true;
      KeepAlive = true;
      EnvironmentVariables.PATH = "${pkgs.git}/bin:/usr/local/bin:/usr/bin:/bin";
    };
  };
}
