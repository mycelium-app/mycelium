{ lib, package }:
{
  enable = lib.mkEnableOption "the Mycelium node daemon";

  package = lib.mkOption {
    type = lib.types.package;
    default = package;
    description = "The mycelium package that provides the node daemon.";
  };

  controlPlane = lib.mkOption {
    type = lib.types.str;
    example = "https://mycelium.example.com";
    description = "URL of the control plane this node joins.";
  };

  joinTokenFile = lib.mkOption {
    type = lib.types.nullOr lib.types.path;
    default = null;
    description = "File holding a single-use join token. When null the node joins with a device code.";
  };

  path = lib.mkOption {
    type = lib.types.listOf lib.types.package;
    default = [ ];
    example = lib.literalExpression "[ pkgs.claude-code pkgs.codex ]";
    description = "Packages whose bin directories are added to the node's PATH, so it finds the agent commands (claude-agent-acp, codex-acp).";
  };
}
