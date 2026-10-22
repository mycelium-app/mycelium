{ pkgs, cfg, defaultConfigPath }:
pkgs.writeShellScript "mycelium-node-launcher" ''
  export PATH="${pkgs.lib.makeBinPath ([ pkgs.git ] ++ cfg.path)}:$PATH:$HOME/.local/bin:$HOME/.nix-profile/bin:/etc/profiles/per-user/$(id -un)/bin:/run/current-system/sw/bin:/usr/local/bin:/usr/bin:/bin"
  config="''${MYCELIUM_NODE_CONFIG:-${defaultConfigPath}}"
  ${pkgs.lib.optionalString (cfg.joinTokenFile != null) ''
    export MYCELIUM_JOIN_TOKEN_FILE=${cfg.joinTokenFile}
  ''}
  export MYCELIUM_NODE_CONFIG="$config"
  exec ${cfg.package}/bin/mycelium node run --control-plane ${pkgs.lib.escapeShellArg cfg.controlPlane}
''
