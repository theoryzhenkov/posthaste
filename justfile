mod template

default:
    @just --list

mod mkdocs

# Phase 2: project setup (run inside the flake devShell, after ./bootstrap.sh)
setup:
    ./setup.sh
