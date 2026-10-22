{ pkgs, cfg, defaultConfigPath }:
pkgs.writeShellScript "mycelium-node-launcher" ''
  config="''${MYCELIUM_NODE_CONFIG:-${defaultConfigPath}}"
  ${pkgs.lib.optionalString (cfg.joinTokenFile != null) ''
    export MYCELIUM_JOIN_TOKEN_FILE=${cfg.joinTokenFile}
  ''}
  if [ ! -f "$config" ]; then
    ${cfg.package}/bin/mycelium node join ${pkgs.lib.escapeShellArg cfg.controlPlane} || exit 1
  fi
  exec ${cfg.package}/bin/mycelium node run
''
