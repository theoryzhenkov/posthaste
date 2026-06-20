use super::*;

#[derive(Debug, Clone)]
pub struct VerifyOptions {
    pub run_root: PathBuf,
    pub registry_path: PathBuf,
    pub argv: Vec<String>,
    pub criteria: SelectionCriteria,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyOutput {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub summary_path: PathBuf,
    pub selected_suite_count: usize,
    pub status: LabStatus,
}

pub fn write_verify_run(
    registry: &SuiteRegistry,
    options: VerifyOptions,
) -> LabResult<VerifyOutput> {
    write_verify_run_with_env(registry, options, std::env::vars())
}

pub fn write_verify_run_with_env<I, K, V>(
    registry: &SuiteRegistry,
    options: VerifyOptions,
    env: I,
) -> LabResult<VerifyOutput>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    let selected_suites = registry.select(&options.criteria)?;
    if selected_suites.is_empty() {
        return Err(LabError::NoSuitesSelected);
    }

    let run_id = new_run_id();
    let run_dir = options.run_root.join(&run_id);
    let config_dir = run_dir.join("state.config");
    let data_dir = run_dir.join("state.data");
    let secrets_dir = run_dir.join("state.secrets");
    let artifact_dir = run_dir.join("artifacts");
    create_dir(&run_dir)?;
    create_dir(&config_dir)?;
    create_dir(&data_dir)?;
    create_dir(&secrets_dir)?;
    create_dir(&artifact_dir)?;

    let profiles = selected_suites
        .iter()
        .filter_map(|suite| suite.profile.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let fixtures = selected_suites
        .iter()
        .filter_map(|suite| suite.fixture.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let declared_artifacts = selected_suites
        .iter()
        .flat_map(|suite| suite.artifacts.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let reproduction_command = reproduction_command(&options.argv);

    let mut suite_results = Vec::new();
    for suite in &selected_suites {
        suite_results.push(execute_suite(
            suite,
            &run_dir,
            &artifact_dir,
            &config_dir,
            &data_dir,
            &secrets_dir,
        )?);
    }

    let status = aggregate_status(&suite_results);
    let first_failure = suite_results
        .iter()
        .find(|result| result.status != LabStatus::Passed)
        .map(|result| result.suite_id.clone());
    let mut artifacts = declared_artifacts;
    for result in &suite_results {
        artifacts.push(result.stdout_path.clone());
        artifacts.push(result.stderr_path.clone());
        artifacts.extend(result.artifact_paths.clone());
    }
    artifacts.sort();
    artifacts.dedup();

    let selection = SelectionRecord::from_criteria(&options.criteria);
    let manifest = LabManifest {
        schema_version: 1,
        run_id: run_id.clone(),
        created_at_unix: OffsetDateTime::now_utc().unix_timestamp(),
        command_id: "cmd.lab.verify.local".to_string(),
        argv: options.argv.clone(),
        reproduction_command: reproduction_command.clone(),
        registry_path: options.registry_path.display().to_string(),
        selected_suites: selected_suites.clone(),
        suite_results: suite_results.clone(),
        selection: selection.clone(),
        commit_id: best_effort_commit_id(),
        platform: PlatformInfo::current(),
        tool_versions: collect_tool_versions(),
        profiles,
        fixtures,
        env_redaction_policy:
            "allowlisted environment names are recorded; secret-like names are redacted".to_string(),
        env: redacted_env_snapshot_from(env),
        process_tree: Vec::new(),
        ports: Vec::new(),
        sockets: Vec::new(),
        artifacts: artifacts.clone(),
    };

    let summary = LabSummary {
        schema_version: 1,
        run_id: run_id.clone(),
        status,
        reason: summary_reason(status, &suite_results),
        selected_suite_count: selected_suites.len(),
        selected_suites: selected_suites
            .iter()
            .map(|suite| suite.id.clone())
            .collect(),
        selection,
        suite_results,
        first_failure,
        reproduction_command,
        important_log_excerpts: important_log_excerpts(&manifest.suite_results),
        artifacts,
    };

    let manifest_path = run_dir.join("manifest.json");
    let summary_path = run_dir.join("summary.json");
    write_json(&manifest_path, &manifest)?;
    write_json(&summary_path, &summary)?;

    Ok(VerifyOutput {
        run_id,
        run_dir,
        manifest_path,
        summary_path,
        selected_suite_count: selected_suites.len(),
        status,
    })
}

pub(crate) fn execute_suite(
    suite: &SelectedSuite,
    run_dir: &Path,
    artifact_dir: &Path,
    config_dir: &Path,
    data_dir: &Path,
    secrets_dir: &Path,
) -> LabResult<SuiteExecutionRecord> {
    let suite_artifact_dir = artifact_dir.join(path_safe_suite_id(&suite.id));
    create_dir(&suite_artifact_dir)?;
    let stdout_path = suite_artifact_dir.join("stdout.log");
    let stderr_path = suite_artifact_dir.join("stderr.log");
    let timeout_seconds = suite
        .timeout_seconds
        .unwrap_or(DEFAULT_SUITE_TIMEOUT_SECONDS);
    let timeout = Duration::from_secs(timeout_seconds);

    let mut command = shell_command(&suite.command);
    configure_command_for_timeout(&mut command);
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("POSTHASTE_LAB_RUN_DIR", run_dir)
        .env("POSTHASTE_CONFIG_ROOT", config_dir)
        .env("POSTHASTE_STATE_ROOT", data_dir)
        .env("POSTHASTE_SECRETS_ROOT", secrets_dir);

    let started = Instant::now();
    let mut child = command.spawn().map_err(|source| LabError::SpawnSuite {
        suite_id: suite.id.clone(),
        source,
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| LabError::CaptureSuiteStream {
            suite_id: suite.id.clone(),
            stream: "stdout",
        })?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| LabError::CaptureSuiteStream {
            suite_id: suite.id.clone(),
            stream: "stderr",
        })?;
    let stdout_reader = read_stream_in_background(stdout);
    let stderr_reader = read_stream_in_background(stderr);

    let mut timed_out = false;
    let exit_status = loop {
        if let Some(status) = child.try_wait().map_err(|source| LabError::RunSuite {
            suite_id: suite.id.clone(),
            action: "poll",
            source,
        })? {
            break Some(status);
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            break Some(terminate_timed_out_child(&mut child, &suite.id)?);
        }
        thread::sleep(Duration::from_millis(50));
    };

    let duration_ms = started.elapsed().as_millis();
    let stdout = join_stream_reader(stdout_reader).map_err(|()| LabError::CaptureSuiteStream {
        suite_id: suite.id.clone(),
        stream: "stdout",
    })?;
    let stderr = join_stream_reader(stderr_reader).map_err(|()| LabError::CaptureSuiteStream {
        suite_id: suite.id.clone(),
        stream: "stderr",
    })?;
    write_bytes(&stdout_path, &stdout)?;
    write_bytes(&stderr_path, &stderr)?;
    let artifact_paths = discover_suite_artifact_paths(&stdout, &stderr, run_dir);

    let (exit_code, signal) = exit_status
        .as_ref()
        .map(exit_status_parts)
        .unwrap_or((None, None));
    let (status, reason) = suite_status(timed_out, exit_code, signal);

    Ok(SuiteExecutionRecord {
        suite_id: suite.id.clone(),
        command: suite.command.clone(),
        status,
        reason,
        exit_code,
        signal,
        timed_out,
        duration_ms,
        timeout_seconds,
        stdout_path: stdout_path.display().to_string(),
        stderr_path: stderr_path.display().to_string(),
        artifact_paths,
    })
}
