use super::*;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SuiteEntry {
    pub level: String,
    pub targets: Vec<String>,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub fixture: Option<String>,
    #[serde(default)]
    pub runners: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    pub command: String,
    #[serde(default, alias = "timeout_seconds")]
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub risk: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuiteRegistry {
    suites: BTreeMap<String, SuiteEntry>,
}

impl SuiteRegistry {
    pub fn load(path: impl AsRef<Path>) -> LabResult<Self> {
        let path = path.as_ref();
        let text = fs::read_to_string(path).map_err(|source| LabError::ReadFile {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_toml_str(&text)
    }

    pub fn from_toml_str(text: &str) -> LabResult<Self> {
        let value = text.parse::<toml::Value>()?;
        let table = value.as_table().ok_or(LabError::MissingSuiteTable)?;
        let suite_value = table.get("suite").ok_or(LabError::MissingSuiteTable)?;
        let suite_table = suite_value.as_table().ok_or(LabError::MissingSuiteTable)?;

        let mut suites = BTreeMap::new();
        for (name, value) in suite_table {
            let nested = value.as_table().ok_or_else(|| LabError::EmptySuiteTable {
                id: format!("suite.{name}"),
            })?;
            flatten_suite_table(&format!("suite.{name}"), nested, &mut suites)?;
        }

        Ok(Self { suites })
    }

    pub fn suites(&self) -> &BTreeMap<String, SuiteEntry> {
        &self.suites
    }

    pub fn select(&self, criteria: &SelectionCriteria) -> LabResult<Vec<SelectedSuite>> {
        if criteria.changed && criteria.changed_paths.is_empty() {
            return Err(LabError::ChangedSelectionNeedsPaths);
        }

        let candidates: Vec<(&String, &SuiteEntry)> = if let Some(id) = &criteria.suite_id {
            validate_lab_id_with_type(id, Some("suite"))?;
            let entry = self
                .suites
                .get(id)
                .ok_or_else(|| LabError::SuiteNotFound(id.clone()))?;
            vec![(id, entry)]
        } else {
            self.suites.iter().collect()
        };

        Ok(candidates
            .into_iter()
            .filter(|(_, entry)| criteria.tags.iter().all(|tag| entry.tags.contains(tag)))
            .filter(|(_, entry)| {
                criteria
                    .targets
                    .iter()
                    .all(|target| entry.targets.contains(target))
            })
            .filter(|(_, entry)| {
                !criteria.changed || suite_matches_changed_paths(entry, &criteria.changed_paths)
            })
            .map(|(id, entry)| SelectedSuite::from_entry(id, entry))
            .collect())
    }
}

pub(crate) fn suite_matches_changed_paths(entry: &SuiteEntry, changed_paths: &[String]) -> bool {
    if changed_paths.iter().any(|path| is_registry_wide_path(path)) {
        return true;
    }

    entry.paths.iter().any(|suite_path| {
        changed_paths
            .iter()
            .any(|changed_path| lab_paths_overlap(suite_path, changed_path))
    })
}

pub(crate) fn is_registry_wide_path(path: &str) -> bool {
    normalize_lab_path(path) == "tools/lab/suites.toml"
}

pub(crate) fn lab_paths_overlap(suite_path: &str, changed_path: &str) -> bool {
    let suite_path = normalize_lab_path(suite_path);
    let changed_path = normalize_lab_path(changed_path);
    if suite_path.is_empty() || changed_path.is_empty() {
        return false;
    }

    if suite_path.ends_with('/') {
        return changed_path.starts_with(&suite_path);
    }
    if changed_path.ends_with('/') {
        return suite_path.starts_with(&changed_path);
    }

    suite_path == changed_path
        || changed_path
            .strip_prefix(&suite_path)
            .is_some_and(|suffix| suffix.starts_with('/'))
        || suite_path
            .strip_prefix(&changed_path)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

pub(crate) fn normalize_lab_path(path: &str) -> String {
    path.trim().trim_start_matches("./").replace('\\', "/")
}

pub(crate) fn flatten_suite_table(
    id: &str,
    table: &toml::map::Map<String, toml::Value>,
    suites: &mut BTreeMap<String, SuiteEntry>,
) -> LabResult<()> {
    if is_suite_leaf(table) {
        validate_lab_id_with_type(id, Some("suite"))?;
        let entry: SuiteEntry = toml::Value::Table(table.clone()).try_into()?;
        validate_suite_entry_refs(&entry)?;
        suites.insert(id.to_string(), entry);
        return Ok(());
    }

    if table.is_empty() {
        return Err(LabError::EmptySuiteTable { id: id.to_string() });
    }

    for (name, value) in table {
        let nested = value.as_table().ok_or_else(|| LabError::EmptySuiteTable {
            id: format!("{id}.{name}"),
        })?;
        flatten_suite_table(&format!("{id}.{name}"), nested, suites)?;
    }

    Ok(())
}

pub(crate) fn is_suite_leaf(table: &toml::map::Map<String, toml::Value>) -> bool {
    table.contains_key("level") || table.contains_key("command") || table.contains_key("targets")
}

pub(crate) fn validate_suite_entry_refs(entry: &SuiteEntry) -> LabResult<()> {
    if let Some(profile) = &entry.profile {
        validate_lab_id_with_type(profile, Some("profile"))?;
    }
    if let Some(fixture) = &entry.fixture {
        validate_lab_id_with_type(fixture, Some("fixture"))?;
    }
    for runner in &entry.runners {
        validate_lab_id_with_type(runner, Some("runner"))?;
    }
    for artifact in &entry.artifacts {
        validate_lab_id(artifact)?;
    }
    Ok(())
}

pub fn validate_lab_id(id: &str) -> LabResult<()> {
    validate_lab_id_with_type(id, None)
}

pub(crate) fn validate_lab_id_with_type(id: &str, expected_type: Option<&str>) -> LabResult<()> {
    if id.is_empty() {
        return Err(invalid_id(id, "id is empty"));
    }
    if !id.contains('.') {
        return Err(invalid_id(
            id,
            "id must include a type and name separated by '.'",
        ));
    }
    if id.starts_with('.') || id.ends_with('.') || id.contains("..") {
        return Err(invalid_id(id, "id has an empty dotted segment"));
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | ':' | '_' | '-'))
    {
        return Err(invalid_id(
            id,
            "id contains unsupported characters; allowed: letters, digits, '.', ':', '_', '-'",
        ));
    }
    for segment in id.split('.') {
        if segment.is_empty() {
            return Err(invalid_id(id, "id has an empty dotted segment"));
        }
        if segment.starts_with(':') || segment.ends_with(':') || segment.contains("::") {
            return Err(invalid_id(id, "id has an invalid ':' segment"));
        }
    }

    let first_segment = id.split('.').next().expect("id contains '.'");
    let id_type = first_segment.split(':').next().unwrap_or(first_segment);
    if !KNOWN_ID_TYPES.contains(&id_type) {
        return Err(invalid_id(id, "id type is not a known lab prefix"));
    }
    if let Some(expected_type) = expected_type {
        if id_type != expected_type {
            return Err(invalid_id(
                id,
                format!("expected {expected_type} id, found {id_type}"),
            ));
        }
    }
    Ok(())
}

pub(crate) fn invalid_id(id: &str, reason: impl Into<String>) -> LabError {
    LabError::InvalidLabId {
        id: id.to_string(),
        reason: reason.into(),
    }
}
