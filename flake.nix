{
  description = "Project";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            git
            jujutsu
            just
            sops
            age
          ];

          shellHook = ''
            FLAKE_ROOT="$PWD"
            while [ "$FLAKE_ROOT" != "/" ] && [ ! -f "$FLAKE_ROOT/flake.nix" ]; do
              FLAKE_ROOT="$(dirname "$FLAKE_ROOT")"
            done

            if [ ! -f "$FLAKE_ROOT/flake.nix" ]; then
              FLAKE_ROOT="$PWD"
            fi

            export FLAKE_ROOT
            export SOPS_AGE_KEY_FILE="$FLAKE_ROOT/.age-key"
          '';
        };
      });
}
