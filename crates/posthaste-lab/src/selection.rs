use super::*;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionCriteria {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suite_id: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub changed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_paths: Vec<String>,
}

impl SelectionCriteria {
    pub fn rationale(&self) -> String {
        let mut parts = Vec::new();
        if self.changed {
            let path_count = self.changed_paths.len();
            parts.push(if path_count == 1 {
                "changed-file selection (1 path)".to_string()
            } else {
                format!("changed-file selection ({path_count} paths)")
            });
        }

        if let Some(id) = &self.suite_id {
            parts.push(format!("explicit suite {id}"));
        }
        if !self.tags.is_empty() {
            parts.push(format!("tags {}", self.tags.join(",")));
        }
        if !self.targets.is_empty() {
            parts.push(format!("targets {}", self.targets.join(",")));
        }
        if parts.is_empty() {
            "all registered suites".to_string()
        } else {
            parts.join(" AND ")
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SelectedSuite {
    pub id: String,
    pub level: String,
    pub targets: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixture: Option<String>,
    pub runners: Vec<String>,
    pub tags: Vec<String>,
    pub paths: Vec<String>,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
    pub artifacts: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
}

impl SelectedSuite {
    pub(crate) fn from_entry(id: &str, entry: &SuiteEntry) -> Self {
        Self {
            id: id.to_string(),
            level: entry.level.clone(),
            targets: entry.targets.clone(),
            profile: entry.profile.clone(),
            fixture: entry.fixture.clone(),
            runners: entry.runners.clone(),
            tags: entry.tags.clone(),
            paths: entry.paths.clone(),
            command: entry.command.clone(),
            timeout_seconds: entry.timeout_seconds,
            artifacts: entry.artifacts.clone(),
            risk: entry.risk.clone(),
        }
    }
}
