---
scope: root
summary: "Project root"
modified: 2026-04-16
reviewed: 2026-04-16
dependents: []
---

# Project Name

> Replace this with a description of your project.

## Setup

Two-phase setup. Requires `nix` and `direnv` installed.

```sh
./bootstrap.sh   # phase 1: creates .envrc, .env, allows direnv
# re-enter the directory so direnv activates the flake
just setup       # phase 2: generates age key, initializes jj
```
