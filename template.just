template_remote := env_var_or_default("TEMPLATE_REMOTE", "template")
template_bookmark := env_var_or_default("TEMPLATE_BOOKMARK", "main")
local_bookmark := env_var_or_default("LOCAL_BOOKMARK", "main")

# Fetch the template remote and rebase the local jj stack onto it.
update remote=template_remote template_bookmark=template_bookmark local_bookmark=local_bookmark:
    #!/usr/bin/env bash
    set -euo pipefail
    target='remote_bookmarks("{{template_bookmark}}", "{{remote}}")'
    local_ref='bookmarks("{{local_bookmark}}")'
    local_name="{{local_bookmark}}"
    tmp="$(mktemp)"
    trap 'rm -f "$tmp"' EXIT

    jj bookmark untrack "{{template_bookmark}}" --remote "{{remote}}" || true
    jj git fetch --remote "{{remote}}"
    jj bookmark untrack "{{template_bookmark}}" --remote "{{remote}}" || true

    if jj log -r "$local_ref & ::$target" --no-graph -T commit_id > "$tmp" && [ -s "$tmp" ]; then
        jj --ignore-immutable bookmark set "$local_name" -r "$target"
        jj --ignore-immutable rebase --skip-emptied -r "($target..@) & mutable()" -d "$target"
    else
        jj --ignore-immutable rebase --skip-emptied -b "$local_ref" -d "$target"
    fi
