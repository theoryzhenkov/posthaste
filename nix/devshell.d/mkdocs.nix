{ pkgs, ... }:
let
  docsPython = pkgs.python3.withPackages (pythonPackages: [
    pythonPackages.mkdocs
    pythonPackages."mkdocs-material"
  ]);
in
{
  packages = [ docsPython ];
}
