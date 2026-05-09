# Update each Copier template layer applied to this project.
update *args:
    #!/usr/bin/env bash
    set -euo pipefail
    shopt -s nullglob

    ordered=(
        .copier-answers.base.yml
        .copier-answers.python.yml
        .copier-answers.skypilot.yml
        .copier-answers.bun.yml
        .copier-answers.mkdocs.yml
    )
    seen=""
    answers=()
    for f in "${ordered[@]}"; do
        if [ -f "$f" ]; then
            answers+=("$f")
            seen="$seen $f"
        fi
    done
    for f in .copier-answers.*.yml; do
        case " $seen " in
            *" $f "*) ;;
            *) answers+=("$f") ;;
        esac
    done

    if [ ${#answers[@]} -eq 0 ]; then
        echo "No .copier-answers.*.yml files found." >&2
        echo "Apply a layer first, e.g. 'just template apply python'." >&2
        exit 1
    fi

    for f in "${answers[@]}"; do
        echo "==> Updating from $f"
        copier update --trust -a "$f" {{args}}
    done

# Apply a Copier template layer into this project.
#
# Usage:
#   just template apply LAYER [COPIER_ARGS...]
#   just template apply-from LAYER SRC [COPIER_ARGS...]
#
# Known layers: base, python, skypilot, bun, mkdocs.
# Extra args are passed to `copier copy`; useful examples:
#   just template apply python --defaults --overwrite -d package_name=my_pkg
apply LAYER *args:
    #!/usr/bin/env bash
    set -euo pipefail

    layer="{{LAYER}}"
    case "$layer" in
        base)
            answers=.copier-answers.base.yml
            src=git@github.com:theoryzhenkov/repo_template.base.git
            ;;
        python)
            answers=.copier-answers.python.yml
            src=git@github.com:theoryzhenkov/repo_template.python.git
            ;;
        skypilot|python_skypilot)
            answers=.copier-answers.skypilot.yml
            src=git@github.com:theoryzhenkov/repo_template.python_skypilot.git
            ;;
        bun)
            answers=.copier-answers.bun.yml
            src=git@github.com:theoryzhenkov/repo_template.bun.git
            ;;
        mkdocs|base_mkdocs)
            answers=.copier-answers.mkdocs.yml
            src=git@github.com:theoryzhenkov/repo_template.base_mkdocs.git
            ;;
        *)
            echo "Unknown layer: $layer" >&2
            echo "Known layers: base, python, skypilot, bun, mkdocs" >&2
            echo "For a custom source, use: just template apply-from LAYER SRC" >&2
            exit 2
            ;;
    esac

    if [ -f "$answers" ]; then
        echo "$answers already exists; use 'just template update' instead." >&2
        exit 1
    fi

    if git rev-parse --is-inside-work-tree >/dev/null 2>&1 && [ -z "${REPO_TEMPLATE_ALLOW_DIRTY:-}" ]; then
        if ! git diff --quiet --ignore-submodules -- || ! git diff --cached --quiet --ignore-submodules -- || [ -n "$(git ls-files --others --exclude-standard)" ]; then
            echo "Destination repository is dirty; commit or stash changes first." >&2
            echo "Set REPO_TEMPLATE_ALLOW_DIRTY=1 to override." >&2
            exit 1
        fi
    fi

    copier copy --trust -a "$answers" {{args}} "$src" .

# Same as `apply`, but with an explicit Copier template source.
apply-from LAYER SRC *args:
    #!/usr/bin/env bash
    set -euo pipefail

    layer="{{LAYER}}"
    src="{{SRC}}"
    case "$layer" in
        base) answers=.copier-answers.base.yml ;;
        python) answers=.copier-answers.python.yml ;;
        skypilot|python_skypilot) answers=.copier-answers.skypilot.yml ;;
        bun) answers=.copier-answers.bun.yml ;;
        mkdocs|base_mkdocs) answers=.copier-answers.mkdocs.yml ;;
        *) answers=".copier-answers.${layer}.yml" ;;
    esac

    if [ -f "$answers" ]; then
        echo "$answers already exists; use 'just template update' instead." >&2
        exit 1
    fi

    if git rev-parse --is-inside-work-tree >/dev/null 2>&1 && [ -z "${REPO_TEMPLATE_ALLOW_DIRTY:-}" ]; then
        if ! git diff --quiet --ignore-submodules -- || ! git diff --cached --quiet --ignore-submodules -- || [ -n "$(git ls-files --others --exclude-standard)" ]; then
            echo "Destination repository is dirty; commit or stash changes first." >&2
            echo "Set REPO_TEMPLATE_ALLOW_DIRTY=1 to override." >&2
            exit 1
        fi
    fi

    copier copy --trust -a "$answers" {{args}} "$src" .

# Backwards-compatible alias for `apply-from` when SRC is supplied.
adopt LAYER SRC *args:
    @just template apply-from "{{LAYER}}" "{{SRC}}" {{args}}

# Adopt a project that was generated from the pre-Copier/v0.0.1 template output.
# This creates a temporary metadata commit so Copier's clean-worktree check passes,
# runs a normal update, then resets the temporary commit while keeping the changes.
legacy-adopt LAYER SRC *args:
    #!/usr/bin/env bash
    set -euo pipefail

    layer="{{LAYER}}"
    src="{{SRC}}"
    case "$layer" in
        base) answers=.copier-answers.base.yml ;;
        python) answers=.copier-answers.python.yml ;;
        skypilot|python_skypilot) answers=.copier-answers.skypilot.yml ;;
        bun) answers=.copier-answers.bun.yml ;;
        mkdocs|base_mkdocs) answers=.copier-answers.mkdocs.yml ;;
        *) answers=".copier-answers.${layer}.yml" ;;
    esac

    if [ -f "$answers" ]; then
        echo "$answers already exists; use 'just template update' instead." >&2
        exit 1
    fi
    if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        echo "legacy-adopt requires a git repository." >&2
        exit 1
    fi
    if ! git diff --quiet --ignore-submodules -- || ! git diff --cached --quiet --ignore-submodules -- || [ -n "$(git ls-files --others --exclude-standard)" ]; then
        echo "Destination repository is dirty; commit or stash changes first." >&2
        exit 1
    fi

    printf '_commit: v0.0.1\n_src_path: %s\n' "$src" > "$answers"
    git add "$answers"
    git -c user.name=repo-template -c user.email=repo-template@localhost \
        commit -qm "repo-template temporary adoption metadata: $layer"

    cleanup() {
        status=$?
        if git rev-parse --verify HEAD^ >/dev/null 2>&1; then
            git reset --mixed HEAD^ >/dev/null || true
        fi
        exit "$status"
    }
    trap cleanup EXIT

    copier update --trust -a "$answers" {{args}}
    trap - EXIT
    git reset --mixed HEAD^ >/dev/null
    echo "Applied legacy adoption for $layer. Review changes, resolve conflicts, then commit."
