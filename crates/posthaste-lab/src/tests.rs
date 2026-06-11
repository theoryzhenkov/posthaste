use super::*;

fn sample_registry_toml() -> &'static str {
    r#"
[suite.api.settings.dev]
level = "integration"
targets = ["daemon"]
profile = "profile.lab.empty.dev"
fixture = "fixture.mail.basic.test"
runners = ["runner.cargo.test.dev"]
tags = ["api", "settings", "fast"]
paths = ["crates/posthaste-server/tests/settings_patch.rs"]
command = "printf 'settings stdout\\n'; printf 'settings stderr\\n' >&2"
timeout_seconds = 5
artifacts = ["log.backend.jsonl.dev"]

[suite.dev.smoke.local]
level = "smoke"
targets = ["dev"]
profile = "profile.lab.empty.local"
runners = ["runner.just.dev.local"]
tags = ["dev", "smoke", "fast"]
paths = ["tools/dev/smoke.sh", "justfile"]
command = "printf 'dev smoke\\n'"
timeout_seconds = 5
artifacts = ["artifact.summary.dev.local"]
"#
}

mod cli_help;
mod registry_selection;
mod run_outputs;
mod run_statuses;
