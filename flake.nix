{
  description = "Mycelium: coding agents on every machine you own, steered from one place";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      workspaceVersion = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
    in
    {
      packages = forAllSystems (system:
        let pkgs = nixpkgs.legacyPackages.${system};
        in {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "mycelium";
            version = workspaceVersion;
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [ "--package" "mycelium-cli" ];
            doCheck = false;
            MYCELIUM_VERSION = "v${workspaceVersion}";
            meta.mainProgram = "mycelium";
          };
        });

      nixosModules.mycelium-node = import ./nix/nixos.nix { inherit self; };
      darwinModules.mycelium-node = import ./nix/darwin.nix { inherit self; };
      homeManagerModules.mycelium-node = import ./nix/home-manager.nix { inherit self; };
    };
}
