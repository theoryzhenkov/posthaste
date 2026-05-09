{
  description = "posthaste";

  inputs = {
    theor-project.url = "git+ssh://git@github.com/theoryzhenkov/repo.nix_project.git";
    nixpkgs.follows = "theor-project/nixpkgs";
  };

  outputs =
    {
      self,
      nixpkgs,
      theor-project,
    }:
    let
      inherit (nixpkgs) lib;
      forEachSystem =
        f: lib.genAttrs theor-project.lib.systems (system: f system (theor-project.lib.mkPkgs system));

      fragmentDir = ./nix/devshell.d;
      loadFragments =
        system: pkgs:
        let
          fragmentFiles =
            if builtins.pathExists fragmentDir then
              lib.sort (a: b: a < b) (
                lib.filter (name: lib.hasSuffix ".nix" name) (builtins.attrNames (builtins.readDir fragmentDir))
              )
            else
              [ ];
        in
        map (name: import (fragmentDir + "/${name}") { inherit pkgs system theor-project; }) fragmentFiles;

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
    in
    {
      devShells = forEachSystem (
        system: pkgs: {
          default =
            let
              fragments = loadFragments system pkgs;
              fragmentPackages = lib.concatMap (fragment: fragment.packages or [ ]) fragments;
              fragmentEnv = lib.foldl' lib.recursiveUpdate { } (map (fragment: fragment.env or { }) fragments);
              fragmentShellHook = lib.concatStringsSep "\n" (map (fragment: fragment.shellHook or "") fragments);
            in
            pkgs.mkShell {
              packages =
                theor-project.lib.toolPackages pkgs [
                  "git"
                  "jujutsu"
                  "just"
                  "sops"
                  "age"
                  "copier"
                ]
                ++ fragmentPackages;

              env = fragmentEnv;

              shellHook = shellHook + "\n" + fragmentShellHook;
            };
        }
      );

      formatter = forEachSystem (_system: pkgs: pkgs.nixfmt);

      checks = forEachSystem (
        system: pkgs: {
          flake-policy =
            if builtins.pathExists ./flake.lock then
              let
                flakeLock = builtins.toFile "flake.lock" (builtins.readFile ./flake.lock);
              in
              pkgs.runCommand "flake-policy" { } ''
                root=$(mktemp -d)
                cp ${flakeLock} "$root/flake.lock"
                ${theor-project.packages.${system}.flakePolicy}/bin/theor-flake-policy "$root"
                touch $out
              ''
            else
              pkgs.runCommand "flake-policy-missing-lock" { } ''
                echo "flake.lock is required for flake policy checks" >&2
                exit 1
              '';
        }
      );
    };
}
