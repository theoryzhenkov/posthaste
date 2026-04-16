default:
    @just --list

# Phase 2: project setup (run inside the flake devShell, after ./bootstrap.sh)
setup:
    ./setup.sh
