# The flake each release publishes beside its artefacts, so Mycelium installs through Nix without
# access to the source. CI fills in the placeholders from the tag and the files it just built.
{
  description = "Mycelium: coding agents on every machine you own, steered from one place";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      pkgs = nixpkgs.legacyPackages.x86_64-linux;
      version = "@VERSION@";
      release = "https://github.com/mycelium-app/mycelium/releases/download/v${version}";
    in
    {
      packages.x86_64-linux = rec {
        mycelium = pkgs.stdenvNoCC.mkDerivation {
          pname = "mycelium";
          inherit version;
          src = pkgs.fetchurl {
            url = "${release}/mycelium-linux-x86_64";
            sha256 = "@BINARY_SHA256@";
          };
          dontUnpack = true;
          installPhase = "install -Dm755 $src $out/bin/mycelium";
          meta.mainProgram = "mycelium";
        };
        mycelium-desktop = pkgs.callPackage ./nix/desktop.nix {
          inherit version;
          distributable = pkgs.fetchurl {
            url = "${release}/mycelium-desktop-linux-x86_64.tar.gz";
            sha256 = "@DESKTOP_SHA256@";
          };
        };
        default = mycelium;
      };

      nixosModules.mycelium-node = import ./nix/nixos.nix { inherit self; };
      homeManagerModules.mycelium-node = import ./nix/home-manager.nix { inherit self; };
    };
}
