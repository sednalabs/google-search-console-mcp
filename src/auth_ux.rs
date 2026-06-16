//! Human-facing authentication helpers for the CLI and setup tools.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{Context, Result, anyhow};
use serde::Serialize;

use crate::config::{
    AuthDoctorArgs, AuthLoginArgs, AuthStatusCliArgs, AuthSubcommand, DEFAULT_SCOPE, Settings,
    WRITE_SCOPE, scope_allows_mutation, scope_allows_read,
};
use crate::contract::redact_secret_text;
use crate::gsc_client::{AuthSource, SearchConsoleClient};

#[derive(Debug, Clone, Serialize)]
struct AuthReport {
    server: &'static str,
    profile: String,
    requested_scope: String,
    auth_source: Option<String>,
    auth_source_candidate: Option<String>,
    config_valid: bool,
    credential_material_detected: bool,
    quota_project_configured: bool,
    detected: CredentialDetection,
    verification: VerificationReport,
    ready: bool,
    next_steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct CredentialDetection {
    gcloud_available: bool,
    gcloud_version: Option<String>,
    adc_file: FilePresence,
    env: EnvPresence,
}

#[derive(Debug, Clone, Serialize)]
struct FilePresence {
    path: Option<String>,
    present: bool,
}

#[derive(Debug, Clone, Serialize)]
struct EnvPresence {
    google_application_credentials: bool,
    google_application_credentials_file_present: bool,
    service_account_json_path: bool,
    service_account_json_path_file_present: bool,
    service_account_json: bool,
    oauth_client_secret_json: bool,
    oauth_client_secret_json_file_present: bool,
    oauth_refresh_token: bool,
    cloudsdk_config: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum VerificationReport {
    NotChecked,
    Ok,
    Failed { error: String },
    ConfigError { error: String },
}

/// Runs the requested auth UX command.
pub async fn run_auth_command(settings: &Settings, command: &AuthSubcommand) -> Result<()> {
    match command {
        AuthSubcommand::Login(args) => {
            ensure_scope_flags_compatible(args.write_scope)?;
            run_login(settings, args).await
        }
        AuthSubcommand::Command(args) => {
            ensure_scope_flags_compatible(args.write_scope)?;
            println!(
                "{}",
                shell_command(&gcloud_login_args(
                    login_scope(settings, args.write_scope),
                    args.headless,
                    args.client_id_file.as_deref()
                ))
            );
            Ok(())
        }
        AuthSubcommand::Status(args) => print_status(settings, args).await,
        AuthSubcommand::Doctor(args) => print_doctor(settings, args).await,
    }
}

fn ensure_scope_flags_compatible(write_scope: bool) -> Result<()> {
    scope_flags_compatible(write_scope, scope_arg_present())
}

fn scope_flags_compatible(write_scope: bool, explicit_scope_arg: bool) -> Result<()> {
    if write_scope && explicit_scope_arg {
        Err(anyhow!(
            "`--write-scope` cannot be combined with an explicit `--scope`; use `--write-scope` alone for operator login, or pass a complete custom scope without `--write-scope`."
        ))
    } else {
        Ok(())
    }
}

pub fn login_command_for_scope(
    scope: &str,
    headless: bool,
    client_id_file: Option<&Path>,
) -> String {
    shell_command(&gcloud_login_args(scope, headless, client_id_file))
}

pub fn auth_login_cli_command(
    scope: &str,
    write_scope: bool,
    headless: bool,
    client_id_file: Option<&Path>,
) -> String {
    let mut args = vec![
        "google-search-console-mcp".to_string(),
        "auth".to_string(),
        "login".to_string(),
    ];
    if write_scope {
        args.push("--write-scope".to_string());
    } else if scope != DEFAULT_SCOPE {
        args.push("--scope".to_string());
        args.push(scope.to_string());
    }
    if headless {
        args.push("--headless".to_string());
    }
    if let Some(path) = client_id_file {
        args.push("--client-id-file".to_string());
        args.push(path.display().to_string());
    }
    shell_command(&args)
}

fn login_scope(settings: &Settings, write_scope: bool) -> &str {
    let ambient_scope = env::var("GOOGLE_SEARCH_CONSOLE_MCP_SCOPE").ok();
    login_scope_from_env_hint(
        settings,
        write_scope,
        ambient_scope.as_deref(),
        scope_arg_present(),
    )
}

fn login_scope_from_env_hint<'a>(
    settings: &'a Settings,
    write_scope: bool,
    ambient_scope: Option<&str>,
    explicit_scope_arg: bool,
) -> &'a str {
    if write_scope {
        WRITE_SCOPE
    } else if !explicit_scope_arg
        && ambient_scope.is_some_and(|scope| scope == settings.scope)
        && settings.scope != DEFAULT_SCOPE
        && (!scope_allows_read(&settings.scope) || scope_allows_mutation(&settings.scope))
    {
        DEFAULT_SCOPE
    } else {
        settings.scope.as_str()
    }
}

fn scope_arg_present() -> bool {
    env::args_os().any(|arg| {
        arg == "--scope"
            || arg
                .to_str()
                .is_some_and(|value| value.starts_with("--scope="))
    })
}

async fn run_login(settings: &Settings, args: &AuthLoginArgs) -> Result<()> {
    let scope = login_scope(settings, args.write_scope).to_string();
    let command_args = gcloud_login_args(&scope, args.headless, args.client_id_file.as_deref());
    let rendered = shell_command(&command_args);

    if args.dry_run {
        println!("{rendered}");
        return Ok(());
    }

    let detection = detect_credentials();
    if !detection.gcloud_available {
        return Err(anyhow!(
            "gcloud was not found on PATH. Install the Google Cloud SDK, then run:\n  {rendered}\n\nService-account deployments can use GOOGLE_APPLICATION_CREDENTIALS or GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH instead."
        ));
    }

    println!("Starting Google Search Console login using Application Default Credentials.");
    println!("Scope: {scope}");
    println!("Command: {rendered}");
    if args.client_id_file.is_none() {
        println!(
            "Tip: Search Console scopes may require a Google OAuth client id file. If Google rejects the scope, rerun with `--client-id-file /path/to/client_id.json`."
        );
    }
    if args.headless {
        println!(
            "Headless mode requested; follow the URL and paste the browser result if gcloud asks."
        );
    }

    let status = ProcessCommand::new(gcloud_command())
        .args(&command_args[1..])
        .status()
        .context("failed to run gcloud")?;
    if !status.success() {
        return Err(anyhow!("gcloud login failed with status {status}"));
    }

    println!("Google login completed.");
    if args.no_verify {
        println!(
            "Verification skipped. Run `google-search-console-mcp auth status --verify-token` when ready."
        );
        for step in post_login_runtime_steps(settings, args.write_scope, &scope) {
            println!("{step}");
        }
        return Ok(());
    }

    let mut verify_settings = settings.clone();
    verify_settings.scope = scope.clone();
    let mut report = build_auth_report(&verify_settings, true).await;
    add_post_login_runtime_steps(settings, args.write_scope, &scope, &mut report);
    print_human_report(&report, true);
    if verification_ok(&report) {
        Ok(())
    } else {
        Err(anyhow!(
            "login completed, but Search Console token verification did not pass"
        ))
    }
}

async fn print_status(settings: &Settings, args: &AuthStatusCliArgs) -> Result<()> {
    let report = build_auth_report(settings, args.verify_token).await;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human_report(&report, false);
    }
    Ok(())
}

async fn print_doctor(settings: &Settings, args: &AuthDoctorArgs) -> Result<()> {
    let report = build_auth_report(settings, args.verify_token).await;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human_report(&report, true);
    }
    Ok(())
}

async fn build_auth_report(settings: &Settings, verify_token: bool) -> AuthReport {
    let detection = detect_credentials();
    let client = SearchConsoleClient::from_settings(settings).await;
    let mut detected_auth_source = None;
    let mut quota_project_configured = settings.quota_project.is_some();
    let verification = match client {
        Ok(client) => {
            detected_auth_source = Some(client.auth_source());
            quota_project_configured = client.quota_project_configured();
            if verify_token {
                match client.verify_token().await {
                    Ok(()) => VerificationReport::Ok,
                    Err(err) => VerificationReport::Failed {
                        error: redact_secret_text(&err.to_string()),
                    },
                }
            } else {
                VerificationReport::NotChecked
            }
        }
        Err(err) => VerificationReport::ConfigError {
            error: redact_secret_text(&err.to_string()),
        },
    };
    let credential_material_detected =
        credential_material_detected(&detection) || settings_credential_material_detected(settings);
    let explicit_credential_needs_repair =
        explicit_credential_config_needs_repair(settings, &detection);
    let auth_source = visible_auth_source(
        detected_auth_source,
        credential_material_detected,
        &verification,
    );
    let auth_source_candidate = detected_auth_source.map(|source| source.as_str().to_string());
    let config_valid = auth_source.is_some()
        && !explicit_credential_needs_repair
        && !matches!(verification, VerificationReport::ConfigError { .. });
    let ready = report_ready(settings, &verification, config_valid);
    let next_steps = next_steps(settings, &detection, &verification, verify_token);

    AuthReport {
        server: "google-search-console-mcp",
        profile: settings.profile.as_str().to_string(),
        requested_scope: settings.scope.clone(),
        auth_source,
        auth_source_candidate,
        config_valid,
        credential_material_detected,
        quota_project_configured,
        detected: detection,
        verification,
        ready,
        next_steps,
    }
}

fn print_human_report(report: &AuthReport, doctor: bool) {
    println!("Google Search Console MCP auth");
    println!("Profile: {}", report.profile);
    println!("Scope: {}", report.requested_scope);
    match (
        report.auth_source.as_deref(),
        report.auth_source_candidate.as_deref(),
    ) {
        (Some(source), _) => println!("Credential source: {source}"),
        (None, Some(candidate)) => {
            println!("Credential source: not verified (candidate: {candidate})")
        }
        (None, None) => println!("Credential source: not configured"),
    }
    println!("Config valid: {}", yes_no(report.config_valid));
    println!(
        "Credential material detected: {}",
        yes_no(report.credential_material_detected)
    );
    println!(
        "Quota project: {}",
        if report.quota_project_configured {
            "configured"
        } else {
            "not configured"
        }
    );
    println!(
        "gcloud: {}",
        report
            .detected
            .gcloud_version
            .as_deref()
            .unwrap_or(if report.detected.gcloud_available {
                "available"
            } else {
                "not found"
            })
    );
    match &report.detected.adc_file.path {
        Some(path) => println!(
            "ADC file: {} ({})",
            if report.detected.adc_file.present {
                "present"
            } else {
                "missing"
            },
            path
        ),
        None => println!("ADC file: unknown"),
    }
    println!(
        "Env credentials: GOOGLE_APPLICATION_CREDENTIALS={}, service-account-path={}, service-account-json={}, oauth-client={}, oauth-refresh-token={}",
        yes_no(report.detected.env.google_application_credentials),
        yes_no(report.detected.env.service_account_json_path),
        yes_no(report.detected.env.service_account_json),
        yes_no(report.detected.env.oauth_client_secret_json),
        yes_no(report.detected.env.oauth_refresh_token),
    );
    match &report.verification {
        VerificationReport::NotChecked => {
            println!("Verification: not checked");
        }
        VerificationReport::Ok => {
            println!("Verification: ok");
        }
        VerificationReport::Failed { error } => {
            println!("Verification: failed");
            println!("Error: {error}");
        }
        VerificationReport::ConfigError { error } => {
            println!("Configuration: invalid");
            println!("Error: {error}");
        }
    }
    println!(
        "Ready: {}",
        if matches!(report.verification, VerificationReport::NotChecked) {
            "not verified"
        } else {
            yes_no(report.ready)
        }
    );
    if doctor || !report.ready {
        println!("Next steps:");
        for step in &report.next_steps {
            println!("- {step}");
        }
    }
}

fn detect_credentials() -> CredentialDetection {
    let gcloud_version = gcloud_version_summary();
    let adc_path = adc_credentials_path();
    CredentialDetection {
        gcloud_available: gcloud_version.is_some(),
        gcloud_version,
        adc_file: FilePresence {
            present: adc_path.as_ref().map(|path| path.exists()).unwrap_or(false),
            path: adc_path.map(|path| path.display().to_string()),
        },
        env: EnvPresence {
            google_application_credentials: env_present("GOOGLE_APPLICATION_CREDENTIALS"),
            google_application_credentials_file_present: path_env_file_present(
                "GOOGLE_APPLICATION_CREDENTIALS",
            ),
            service_account_json_path: env_present(
                "GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH",
            ),
            service_account_json_path_file_present: path_env_file_present(
                "GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH",
            ),
            service_account_json: env_present("GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON"),
            oauth_client_secret_json: env_present(
                "GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_CLIENT_SECRET_JSON",
            ),
            oauth_client_secret_json_file_present: path_env_file_present(
                "GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_CLIENT_SECRET_JSON",
            ),
            oauth_refresh_token: env_present("GOOGLE_SEARCH_CONSOLE_MCP_OAUTH_REFRESH_TOKEN"),
            cloudsdk_config: env_present("CLOUDSDK_CONFIG"),
        },
    }
}

pub fn local_credential_material_detected() -> bool {
    credential_material_detected(&detect_credentials())
}

fn credential_material_detected(detection: &CredentialDetection) -> bool {
    detection.adc_file.present
        || detection.env.google_application_credentials_file_present
        || detection.env.service_account_json_path_file_present
        || detection.env.service_account_json
        || (detection.env.oauth_client_secret_json_file_present
            && detection.env.oauth_refresh_token)
}

fn settings_credential_material_detected(settings: &Settings) -> bool {
    settings
        .service_account_json_path
        .as_deref()
        .is_some_and(|path| Path::new(path).is_file())
        || settings
            .service_account_json
            .as_deref()
            .is_some_and(|json| !json.is_empty())
        || (settings
            .oauth_client_secret_json
            .as_deref()
            .is_some_and(|path| Path::new(path).is_file())
            && settings
                .oauth_refresh_token
                .as_deref()
                .is_some_and(|token| !token.is_empty()))
}

fn explicit_credential_env_detected(detection: &CredentialDetection) -> bool {
    detection.env.google_application_credentials
        || detection.env.service_account_json_path
        || detection.env.service_account_json
        || detection.env.oauth_client_secret_json
        || detection.env.oauth_refresh_token
}

fn explicit_credential_config_detected(
    settings: &Settings,
    detection: &CredentialDetection,
) -> bool {
    explicit_credential_env_detected(detection)
        || settings.service_account_json_path.is_some()
        || settings.service_account_json.is_some()
        || settings.oauth_client_secret_json.is_some()
        || settings.oauth_refresh_token.is_some()
}

fn explicit_credential_material_detected(
    settings: &Settings,
    detection: &CredentialDetection,
) -> bool {
    detection.env.google_application_credentials_file_present
        || detection.env.service_account_json_path_file_present
        || detection.env.service_account_json
        || (detection.env.oauth_client_secret_json_file_present
            && detection.env.oauth_refresh_token)
        || settings_credential_material_detected(settings)
}

fn explicit_credential_config_needs_repair(
    settings: &Settings,
    detection: &CredentialDetection,
) -> bool {
    explicit_credential_config_detected(settings, detection)
        && !explicit_credential_material_detected(settings, detection)
}

fn visible_auth_source(
    detected_auth_source: Option<AuthSource>,
    credential_material_detected: bool,
    verification: &VerificationReport,
) -> Option<String> {
    match detected_auth_source {
        Some(AuthSource::GoogleDefaultProviderChain)
            if !credential_material_detected && !matches!(verification, VerificationReport::Ok) =>
        {
            None
        }
        Some(source) => Some(source.as_str().to_string()),
        None => None,
    }
}

fn report_ready(
    settings: &Settings,
    verification: &VerificationReport,
    config_valid: bool,
) -> bool {
    config_valid
        && matches!(verification, VerificationReport::Ok)
        && scope_allows_read(&settings.scope)
        && (!settings.profile.allows_mutation() || scope_allows_mutation(&settings.scope))
}

fn verification_ok(report: &AuthReport) -> bool {
    matches!(report.verification, VerificationReport::Ok)
}

fn add_post_login_runtime_steps(
    original_settings: &Settings,
    write_scope_login: bool,
    login_scope: &str,
    report: &mut AuthReport,
) {
    let ambient_scope = env::var("GOOGLE_SEARCH_CONSOLE_MCP_SCOPE").ok();
    add_post_login_runtime_steps_with_env(
        original_settings,
        write_scope_login,
        login_scope,
        ambient_scope.as_deref(),
        report,
    );
}

fn add_post_login_runtime_steps_with_env(
    original_settings: &Settings,
    write_scope_login: bool,
    login_scope: &str,
    ambient_scope: Option<&str>,
    report: &mut AuthReport,
) {
    let runtime_steps = post_login_runtime_steps_with_env(
        original_settings,
        write_scope_login,
        login_scope,
        ambient_scope,
    );
    for step in runtime_steps.into_iter().rev() {
        report.next_steps.insert(0, step);
    }
    if runtime_scope_needs_repair(write_scope_login, login_scope, ambient_scope) {
        report.ready = false;
    }
    if original_settings.profile.allows_mutation()
        && !scope_allows_mutation(&original_settings.scope)
    {
        report.ready = false;
        let operator_scope_step = operator_scope_step();
        if !report
            .next_steps
            .iter()
            .any(|step| step == &operator_scope_step)
        {
            report.next_steps.insert(0, operator_scope_step);
        }
    }
}

fn post_login_runtime_steps(
    original_settings: &Settings,
    write_scope_login: bool,
    login_scope: &str,
) -> Vec<String> {
    let ambient_scope = env::var("GOOGLE_SEARCH_CONSOLE_MCP_SCOPE").ok();
    post_login_runtime_steps_with_env(
        original_settings,
        write_scope_login,
        login_scope,
        ambient_scope.as_deref(),
    )
}

fn post_login_runtime_steps_with_env(
    original_settings: &Settings,
    write_scope_login: bool,
    login_scope: &str,
    ambient_scope: Option<&str>,
) -> Vec<String> {
    let mut steps = Vec::new();
    if runtime_scope_needs_repair(write_scope_login, login_scope, ambient_scope) {
        steps.push(runtime_scope_step(login_scope));
    }
    if write_scope_login {
        if !original_settings.profile.allows_mutation() {
            steps.push(
                "Set GOOGLE_SEARCH_CONSOLE_MCP_PROFILE=operator or start the MCP server with `--profile operator` before using operator tools."
                    .to_string(),
            );
        }
        if !scope_allows_mutation(&original_settings.scope) {
            steps.push(operator_scope_step());
        }
    }
    steps
}

fn runtime_scope_needs_repair(
    write_scope_login: bool,
    login_scope: &str,
    ambient_scope: Option<&str>,
) -> bool {
    !write_scope_login
        && ambient_scope
            .filter(|scope| !scope.is_empty())
            .is_some_and(|scope| scope != login_scope)
}

fn runtime_scope_step(scope: &str) -> String {
    format!(
        "Unset GOOGLE_SEARCH_CONSOLE_MCP_SCOPE, set GOOGLE_SEARCH_CONSOLE_MCP_SCOPE={scope}, or update any MCP launcher `--scope` argument before starting the MCP server; stale scope configuration overrides the login scope."
    )
}

fn operator_scope_step() -> String {
    format!(
        "Set GOOGLE_SEARCH_CONSOLE_MCP_SCOPE={WRITE_SCOPE} or start the MCP server with `--scope {WRITE_SCOPE}` before running operator tools."
    )
}

fn next_steps(
    settings: &Settings,
    detection: &CredentialDetection,
    verification: &VerificationReport,
    verify_token: bool,
) -> Vec<String> {
    let operator_missing_write_scope =
        settings.profile.allows_mutation() && !scope_allows_mutation(&settings.scope);
    let missing_search_console_scope = !scope_allows_read(&settings.scope);
    let read_scope_step = format!(
        "Set GOOGLE_SEARCH_CONSOLE_MCP_SCOPE={DEFAULT_SCOPE} or start the MCP server with `--scope {DEFAULT_SCOPE}` for read-only tools; use {WRITE_SCOPE} or `--scope {WRITE_SCOPE}` for operator tools."
    );
    let login_command = if operator_missing_write_scope {
        "google-search-console-mcp auth login --write-scope"
    } else if missing_search_console_scope {
        "google-search-console-mcp auth login --scope https://www.googleapis.com/auth/webmasters.readonly"
    } else {
        "google-search-console-mcp auth login"
    };
    match verification {
        VerificationReport::Ok => {
            let mut steps = Vec::new();
            if explicit_credential_config_needs_repair(settings, detection) {
                steps.push("Fix or clear explicit credential configuration before browser login; it takes precedence over Application Default Credentials.".to_string());
            }
            if missing_search_console_scope {
                steps.push(read_scope_step.clone());
            }
            if operator_missing_write_scope {
                steps.push(format!(
                    "Set GOOGLE_SEARCH_CONSOLE_MCP_SCOPE={WRITE_SCOPE} or start the MCP server with `--scope {WRITE_SCOPE}` before running operator tools."
                ));
            }
            steps.push(
                "Restart MCP clients that keep long-lived stdio server processes.".to_string(),
            );
            steps.push(
                "Call gsc_sites_list to discover exact Search Console property strings."
                    .to_string(),
            );
            steps
        }
        VerificationReport::NotChecked if !verify_token => {
            if credential_material_detected(detection)
                || settings_credential_material_detected(settings)
            {
                let mut steps = Vec::new();
                if explicit_credential_config_needs_repair(settings, detection) {
                    steps.push("Fix or clear explicit credential configuration before browser login; it takes precedence over Application Default Credentials.".to_string());
                }
                if missing_search_console_scope {
                    steps.push(read_scope_step.clone());
                }
                if operator_missing_write_scope {
                    steps.push(format!(
                        "Set GOOGLE_SEARCH_CONSOLE_MCP_SCOPE={WRITE_SCOPE} or start the MCP server with `--scope {WRITE_SCOPE}`, then run `google-search-console-mcp auth login --write-scope` before using operator tools."
                    ));
                }
                steps.extend([
                    "Run `google-search-console-mcp auth status --verify-token` to prove token acquisition.".to_string(),
                    "Then restart the MCP client and call gsc_sites_list.".to_string(),
                ]);
                steps
            } else {
                let mut steps = vec![
                    format!("Run `{login_command}` for browser login."),
                    "Then run `google-search-console-mcp auth status --verify-token` to prove token acquisition.".to_string(),
                    "Restart the MCP client and call gsc_sites_list.".to_string(),
                ];
                if explicit_credential_config_detected(settings, detection) {
                    steps.insert(0, "Fix or clear explicit credential configuration before browser login; it takes precedence over Application Default Credentials.".to_string());
                }
                if missing_search_console_scope {
                    steps.insert(0, read_scope_step.clone());
                }
                if operator_missing_write_scope {
                    steps.insert(
                        1,
                        format!(
                            "Set GOOGLE_SEARCH_CONSOLE_MCP_SCOPE={WRITE_SCOPE} or start the MCP server with `--scope {WRITE_SCOPE}` before using operator mode."
                        ),
                    );
                }
                steps
            }
        }
        VerificationReport::Failed { error } => {
            let mut steps = Vec::new();
            if missing_search_console_scope {
                steps.push(read_scope_step.clone());
            }
            if explicit_credential_config_detected(settings, detection) {
                steps.push("Fix or clear explicit credential configuration before browser login; it takes precedence over Application Default Credentials.".to_string());
            }
            if verification_needs_quota_project(error) {
                steps.push("Set an ADC quota project with `gcloud auth application-default set-quota-project YOUR_PROJECT`; the project must have the Search Console API enabled and your account must be allowed to use it for quota.".to_string());
                steps.push(
                    "Then rerun `google-search-console-mcp auth status --verify-token`."
                        .to_string(),
                );
            } else if !detection.gcloud_available {
                steps.push("Install the Google Cloud SDK, or configure a service-account file with GOOGLE_APPLICATION_CREDENTIALS.".to_string());
            } else {
                steps.push(format!("Run `{login_command}` for browser login."));
                if operator_missing_write_scope {
                    steps.push(format!(
                        "Set GOOGLE_SEARCH_CONSOLE_MCP_SCOPE={WRITE_SCOPE} or start the MCP server with `--scope {WRITE_SCOPE}` before using operator mode."
                    ));
                } else {
                    steps.push("Use `google-search-console-mcp auth login --write-scope` only when preparing operator tools.".to_string());
                }
            }
            steps.push("For unattended deployments, prefer GOOGLE_APPLICATION_CREDENTIALS or GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH.".to_string());
            steps
        }
        VerificationReport::ConfigError { .. } => {
            let mut steps = Vec::new();
            if missing_search_console_scope {
                steps.push(read_scope_step);
            }
            if explicit_credential_config_detected(settings, detection) {
                steps.push("Fix or clear malformed explicit credential configuration before browser login; it takes precedence over Application Default Credentials.".to_string());
            }
            if !detection.gcloud_available {
                steps.push("Install the Google Cloud SDK for browser login, or configure a valid service-account file with GOOGLE_APPLICATION_CREDENTIALS.".to_string());
            } else {
                steps.push(format!("Run `{login_command}` after explicit credential configuration is fixed or cleared."));
                if operator_missing_write_scope {
                    steps.push(format!(
                        "Set GOOGLE_SEARCH_CONSOLE_MCP_SCOPE={WRITE_SCOPE} or start the MCP server with `--scope {WRITE_SCOPE}` before using operator mode."
                    ));
                }
            }
            steps.push("For unattended deployments, prefer GOOGLE_APPLICATION_CREDENTIALS or GOOGLE_SEARCH_CONSOLE_MCP_SERVICE_ACCOUNT_JSON_PATH.".to_string());
            steps
        }
        VerificationReport::NotChecked => vec![
            "Run `google-search-console-mcp auth status --verify-token` to prove token acquisition."
                .to_string(),
        ],
    }
}

fn verification_needs_quota_project(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("quota project")
        || lower.contains("quota_project")
        || lower.contains("x-goog-user-project")
        || lower.contains("service_disabled")
}

pub const GCLOUD_ADC_REQUIRED_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

pub fn adc_login_scopes(scope: &str) -> String {
    let mut scopes = Vec::new();
    let mut has_required_scope = false;

    for item in scope
        .split([',', ' ', '\n', '\t'])
        .filter(|item| !item.is_empty())
    {
        if item == GCLOUD_ADC_REQUIRED_SCOPE {
            has_required_scope = true;
        }
        if !scopes.iter().any(|existing| existing == &item) {
            scopes.push(item);
        }
    }

    if !has_required_scope {
        scopes.insert(0, GCLOUD_ADC_REQUIRED_SCOPE);
    }

    scopes.join(",")
}

fn gcloud_login_args(scope: &str, headless: bool, client_id_file: Option<&Path>) -> Vec<String> {
    let login_scopes = adc_login_scopes(scope);
    let mut args = vec![
        "gcloud".to_string(),
        "auth".to_string(),
        "application-default".to_string(),
        "login".to_string(),
        format!("--scopes={login_scopes}"),
    ];
    if headless {
        args.push("--no-launch-browser".to_string());
    }
    if let Some(path) = client_id_file {
        args.push("--client-id-file".to_string());
        args.push(path.display().to_string());
    }
    args
}

fn shell_command(args: &[String]) -> String {
    args.iter()
        .map(|arg| shell_word(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_word(arg: &str) -> String {
    if !arg.is_empty()
        && arg
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':' | '='))
    {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', r#"'\''"#))
    }
}

fn gcloud_command() -> &'static str {
    if cfg!(windows) {
        "gcloud.cmd"
    } else {
        "gcloud"
    }
}

fn gcloud_version_summary() -> Option<String> {
    let output = ProcessCommand::new(gcloud_command())
        .arg("--version")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
}

fn adc_credentials_path() -> Option<PathBuf> {
    if let Some(config) = env::var_os("CLOUDSDK_CONFIG").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(config).join("application_default_credentials.json"));
    }
    if cfg!(windows) {
        return env::var_os("APPDATA")
            .filter(|value| !value.is_empty())
            .map(|appdata| {
                PathBuf::from(appdata)
                    .join("gcloud")
                    .join("application_default_credentials.json")
            });
    }
    home_dir().map(|home| {
        home.join(".config")
            .join("gcloud")
            .join("application_default_credentials.json")
    })
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("USERPROFILE")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
}

fn env_present(name: &str) -> bool {
    env::var_os(name)
        .map(|value| !value.is_empty())
        .unwrap_or(false)
}

fn path_env_file_present(name: &str) -> bool {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(|value| PathBuf::from(value).is_file())
        .unwrap_or(false)
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CapabilityProfile, CliCommand, DEFAULT_SCOPE};

    #[test]
    fn login_command_defaults_to_read_only_scope() {
        let settings = test_settings(DEFAULT_SCOPE);

        assert_eq!(
            login_command_for_scope(login_scope(&settings, false), false, None),
            "gcloud auth application-default login '--scopes=https://www.googleapis.com/auth/cloud-platform,https://www.googleapis.com/auth/webmasters.readonly'"
        );
    }

    #[test]
    fn gcloud_command_matches_platform_executable() {
        if cfg!(windows) {
            assert_eq!(gcloud_command(), "gcloud.cmd");
        } else {
            assert_eq!(gcloud_command(), "gcloud");
        }
    }

    #[test]
    fn shell_word_quotes_empty_arguments() {
        assert_eq!(shell_word(""), "''");
    }

    #[test]
    fn adc_login_scopes_include_gcloud_required_scope_once() {
        assert_eq!(
            adc_login_scopes(DEFAULT_SCOPE),
            "https://www.googleapis.com/auth/cloud-platform,https://www.googleapis.com/auth/webmasters.readonly"
        );
        assert_eq!(
            adc_login_scopes(
                "https://www.googleapis.com/auth/webmasters.readonly,https://www.googleapis.com/auth/cloud-platform"
            ),
            "https://www.googleapis.com/auth/webmasters.readonly,https://www.googleapis.com/auth/cloud-platform"
        );
    }

    #[test]
    fn login_scope_repairs_ambient_bad_scope() {
        let settings = test_settings("https://www.googleapis.com/auth/drive");

        assert_eq!(
            login_scope_from_env_hint(
                &settings,
                false,
                Some("https://www.googleapis.com/auth/drive"),
                false
            ),
            DEFAULT_SCOPE
        );
    }

    #[test]
    fn login_scope_repairs_ambient_write_scope_without_write_flag() {
        let settings = test_settings(WRITE_SCOPE);

        assert_eq!(
            login_scope_from_env_hint(&settings, false, Some(WRITE_SCOPE), false),
            DEFAULT_SCOPE
        );
    }

    #[test]
    fn login_scope_preserves_explicit_custom_scope_without_env_match() {
        let settings = test_settings("https://www.googleapis.com/auth/drive");

        assert_eq!(
            login_scope_from_env_hint(&settings, false, None, false),
            "https://www.googleapis.com/auth/drive"
        );
    }

    #[test]
    fn login_scope_preserves_explicit_custom_scope_even_when_env_matches() {
        let custom_scope = "https://www.googleapis.com/auth/webmasters.readonly https://www.googleapis.com/auth/userinfo.email";
        let settings = test_settings(custom_scope);

        assert_eq!(
            login_scope_from_env_hint(&settings, false, Some(custom_scope), true),
            custom_scope
        );
    }

    #[test]
    fn login_scope_preserves_env_custom_read_scope() {
        let custom_scope = "https://www.googleapis.com/auth/webmasters.readonly https://www.googleapis.com/auth/userinfo.email";
        let settings = test_settings(custom_scope);

        assert_eq!(
            login_scope_from_env_hint(&settings, false, Some(custom_scope), false),
            custom_scope
        );
    }

    #[test]
    fn login_scope_write_flag_overrides_ambient_scope() {
        let settings = test_settings("https://www.googleapis.com/auth/drive");

        assert_eq!(
            login_scope_from_env_hint(
                &settings,
                true,
                Some("https://www.googleapis.com/auth/drive"),
                false
            ),
            WRITE_SCOPE
        );
    }

    #[test]
    fn login_command_supports_write_headless_and_client_file() {
        let command =
            login_command_for_scope(WRITE_SCOPE, true, Some(Path::new("/tmp/client id.json")));

        assert_eq!(
            command,
            "gcloud auth application-default login '--scopes=https://www.googleapis.com/auth/cloud-platform,https://www.googleapis.com/auth/webmasters' --no-launch-browser --client-id-file '/tmp/client id.json'"
        );
    }

    #[test]
    fn write_scope_rejects_explicit_custom_scope_combination() {
        assert!(scope_flags_compatible(true, true).is_err());
        assert!(scope_flags_compatible(true, false).is_ok());
        assert!(scope_flags_compatible(false, true).is_ok());
    }

    #[test]
    fn auth_login_cli_command_includes_copyable_flags() {
        let command = auth_login_cli_command(
            DEFAULT_SCOPE,
            true,
            true,
            Some(Path::new("/tmp/client id.json")),
        );

        assert_eq!(
            command,
            "google-search-console-mcp auth login --write-scope --headless --client-id-file '/tmp/client id.json'"
        );
    }

    #[test]
    fn auth_login_cli_command_includes_custom_scope_without_write_flag() {
        let command = auth_login_cli_command(
            "https://www.googleapis.com/auth/webmasters.readonly extra",
            false,
            false,
            None,
        );

        assert_eq!(
            command,
            "google-search-console-mcp auth login --scope 'https://www.googleapis.com/auth/webmasters.readonly extra'"
        );
    }

    #[test]
    fn next_steps_prefer_login_when_verification_fails_and_gcloud_exists() {
        let detection = CredentialDetection {
            gcloud_available: true,
            gcloud_version: Some("Google Cloud SDK 999.0.0".to_string()),
            adc_file: FilePresence {
                path: Some("/tmp/adc.json".to_string()),
                present: false,
            },
            env: EnvPresence {
                google_application_credentials: false,
                google_application_credentials_file_present: false,
                service_account_json_path: false,
                service_account_json_path_file_present: false,
                service_account_json: false,
                oauth_client_secret_json: false,
                oauth_client_secret_json_file_present: false,
                oauth_refresh_token: false,
                cloudsdk_config: false,
            },
        };

        let steps = next_steps(
            &test_settings(DEFAULT_SCOPE),
            &detection,
            &VerificationReport::Failed {
                error: "no token".to_string(),
            },
            true,
        );

        assert!(steps.iter().any(|step| step.contains("auth login")));
    }

    #[test]
    fn next_steps_call_out_missing_quota_project() {
        let detection = CredentialDetection {
            gcloud_available: true,
            gcloud_version: Some("Google Cloud SDK 999.0.0".to_string()),
            adc_file: FilePresence {
                path: Some("/tmp/adc.json".to_string()),
                present: true,
            },
            env: EnvPresence {
                google_application_credentials: false,
                google_application_credentials_file_present: false,
                service_account_json_path: false,
                service_account_json_path_file_present: false,
                service_account_json: false,
                oauth_client_secret_json: false,
                oauth_client_secret_json_file_present: false,
                oauth_refresh_token: false,
                cloudsdk_config: false,
            },
        };

        let steps = next_steps(
            &test_settings(DEFAULT_SCOPE),
            &detection,
            &VerificationReport::Failed {
                error: "PERMISSION_DENIED: local Application Default Credentials requires a quota project; SERVICE_DISABLED".to_string(),
            },
            true,
        );

        assert!(
            steps
                .iter()
                .any(|step| step.contains("set-quota-project YOUR_PROJECT"))
        );
        assert!(!steps.iter().any(|step| step.contains("auth login")));
    }

    #[test]
    fn next_steps_use_write_login_for_operator_without_write_scope() {
        let mut settings = test_settings("https://www.googleapis.com/auth/drive");
        settings.profile = CapabilityProfile::Operator;
        let detection = CredentialDetection {
            gcloud_available: true,
            gcloud_version: Some("Google Cloud SDK 999.0.0".to_string()),
            adc_file: FilePresence {
                path: Some("/tmp/adc.json".to_string()),
                present: false,
            },
            env: EnvPresence {
                google_application_credentials: false,
                google_application_credentials_file_present: false,
                service_account_json_path: false,
                service_account_json_path_file_present: false,
                service_account_json: false,
                oauth_client_secret_json: false,
                oauth_client_secret_json_file_present: false,
                oauth_refresh_token: false,
                cloudsdk_config: false,
            },
        };

        let steps = next_steps(
            &settings,
            &detection,
            &VerificationReport::NotChecked,
            false,
        );

        assert!(
            steps
                .iter()
                .any(|step| step.contains("auth login --write-scope"))
        );
        assert!(
            steps
                .iter()
                .any(|step| step.contains("GOOGLE_SEARCH_CONSOLE_MCP_SCOPE"))
        );
    }

    #[test]
    fn config_error_steps_prioritize_clearing_explicit_credential_env() {
        let detection = CredentialDetection {
            gcloud_available: true,
            gcloud_version: Some("Google Cloud SDK 999.0.0".to_string()),
            adc_file: FilePresence {
                path: Some("/tmp/adc.json".to_string()),
                present: false,
            },
            env: EnvPresence {
                google_application_credentials: false,
                google_application_credentials_file_present: false,
                service_account_json_path: false,
                service_account_json_path_file_present: false,
                service_account_json: true,
                oauth_client_secret_json: false,
                oauth_client_secret_json_file_present: false,
                oauth_refresh_token: false,
                cloudsdk_config: false,
            },
        };

        let steps = next_steps(
            &test_settings(DEFAULT_SCOPE),
            &detection,
            &VerificationReport::ConfigError {
                error: "bad json".to_string(),
            },
            true,
        );

        assert!(
            steps
                .first()
                .is_some_and(|step| step.contains("Fix or clear malformed explicit credential"))
        );
    }

    #[test]
    fn ready_requires_write_scope_for_operator_profile() {
        let mut settings = test_settings(DEFAULT_SCOPE);
        settings.profile = CapabilityProfile::Operator;

        assert!(!report_ready(&settings, &VerificationReport::Ok, true));

        settings.scope = WRITE_SCOPE.to_string();
        assert!(report_ready(&settings, &VerificationReport::Ok, true));
        assert!(!report_ready(&settings, &VerificationReport::Ok, false));
    }

    #[test]
    fn ready_requires_search_console_scope_for_read_only_profile() {
        let settings = test_settings("https://www.googleapis.com/auth/drive");

        assert!(!report_ready(&settings, &VerificationReport::Ok, true));
    }

    #[test]
    fn post_login_write_scope_reports_operator_runtime_steps() {
        let settings = test_settings(DEFAULT_SCOPE);
        let mut report = AuthReport {
            server: "google-search-console-mcp",
            profile: CapabilityProfile::ReadOnly.as_str().to_string(),
            requested_scope: WRITE_SCOPE.to_string(),
            auth_source: Some("google_default_provider_chain".to_string()),
            auth_source_candidate: Some("google_default_provider_chain".to_string()),
            config_valid: true,
            credential_material_detected: true,
            quota_project_configured: false,
            detected: CredentialDetection {
                gcloud_available: true,
                gcloud_version: Some("Google Cloud SDK 999.0.0".to_string()),
                adc_file: FilePresence {
                    path: Some("/tmp/adc.json".to_string()),
                    present: true,
                },
                env: EnvPresence {
                    google_application_credentials: false,
                    google_application_credentials_file_present: false,
                    service_account_json_path: false,
                    service_account_json_path_file_present: false,
                    service_account_json: false,
                    oauth_client_secret_json: false,
                    oauth_client_secret_json_file_present: false,
                    oauth_refresh_token: false,
                    cloudsdk_config: false,
                },
            },
            verification: VerificationReport::Ok,
            ready: true,
            next_steps: Vec::new(),
        };

        add_post_login_runtime_steps(&settings, true, WRITE_SCOPE, &mut report);

        assert!(
            report
                .next_steps
                .iter()
                .any(|step| step.contains("GOOGLE_SEARCH_CONSOLE_MCP_PROFILE=operator"))
        );
        assert!(
            report
                .next_steps
                .iter()
                .any(|step| step.contains("--profile operator"))
        );
        assert!(
            report
                .next_steps
                .iter()
                .any(|step| step.contains("GOOGLE_SEARCH_CONSOLE_MCP_SCOPE"))
        );
        assert!(
            report
                .next_steps
                .iter()
                .any(|step| step.contains("--scope"))
        );
        assert!(verification_ok(&report));
    }

    #[test]
    fn no_verify_write_scope_uses_same_operator_runtime_steps() {
        let settings = test_settings(DEFAULT_SCOPE);
        let steps = post_login_runtime_steps(&settings, true, WRITE_SCOPE);

        assert!(
            steps
                .iter()
                .any(|step| step.contains("GOOGLE_SEARCH_CONSOLE_MCP_PROFILE=operator"))
        );
        assert!(steps.iter().any(|step| step.contains("--profile operator")));
        assert!(
            steps
                .iter()
                .any(|step| step.contains("GOOGLE_SEARCH_CONSOLE_MCP_SCOPE"))
        );
        assert!(steps.iter().any(|step| step.contains("--scope")));
    }

    #[test]
    fn adc_without_material_is_not_reported_as_configured_before_verification() {
        assert_eq!(
            visible_auth_source(
                Some(AuthSource::GoogleDefaultProviderChain),
                false,
                &VerificationReport::NotChecked,
            ),
            None
        );
    }

    #[test]
    fn verified_adc_can_still_report_auth_source_without_local_file() {
        assert_eq!(
            visible_auth_source(
                Some(AuthSource::GoogleDefaultProviderChain),
                false,
                &VerificationReport::Ok,
            ),
            Some("google_default_provider_chain".to_string())
        );
    }

    #[test]
    fn missing_path_env_does_not_count_as_credential_material() {
        let detection = CredentialDetection {
            gcloud_available: true,
            gcloud_version: Some("Google Cloud SDK 999.0.0".to_string()),
            adc_file: FilePresence {
                path: Some("/tmp/adc.json".to_string()),
                present: false,
            },
            env: EnvPresence {
                google_application_credentials: true,
                google_application_credentials_file_present: false,
                service_account_json_path: true,
                service_account_json_path_file_present: false,
                service_account_json: false,
                oauth_client_secret_json: true,
                oauth_client_secret_json_file_present: false,
                oauth_refresh_token: true,
                cloudsdk_config: false,
            },
        };

        assert!(!credential_material_detected(&detection));
    }

    #[test]
    fn missing_path_env_is_called_out_before_browser_login() {
        let detection = CredentialDetection {
            gcloud_available: true,
            gcloud_version: Some("Google Cloud SDK 999.0.0".to_string()),
            adc_file: FilePresence {
                path: Some("/tmp/adc.json".to_string()),
                present: false,
            },
            env: EnvPresence {
                google_application_credentials: true,
                google_application_credentials_file_present: false,
                service_account_json_path: false,
                service_account_json_path_file_present: false,
                service_account_json: false,
                oauth_client_secret_json: false,
                oauth_client_secret_json_file_present: false,
                oauth_refresh_token: false,
                cloudsdk_config: false,
            },
        };

        let steps = next_steps(
            &test_settings(DEFAULT_SCOPE),
            &detection,
            &VerificationReport::NotChecked,
            false,
        );

        assert!(
            steps
                .first()
                .is_some_and(|step| step.contains("Fix or clear explicit credential"))
        );
    }

    #[test]
    fn missing_explicit_env_is_called_out_even_when_adc_file_exists() {
        let detection = CredentialDetection {
            gcloud_available: true,
            gcloud_version: Some("Google Cloud SDK 999.0.0".to_string()),
            adc_file: FilePresence {
                path: Some("/tmp/adc.json".to_string()),
                present: true,
            },
            env: EnvPresence {
                google_application_credentials: true,
                google_application_credentials_file_present: false,
                service_account_json_path: false,
                service_account_json_path_file_present: false,
                service_account_json: false,
                oauth_client_secret_json: false,
                oauth_client_secret_json_file_present: false,
                oauth_refresh_token: false,
                cloudsdk_config: false,
            },
        };
        let settings = test_settings(DEFAULT_SCOPE);

        assert!(credential_material_detected(&detection));
        assert!(explicit_credential_config_needs_repair(
            &settings, &detection
        ));

        let steps = next_steps(
            &settings,
            &detection,
            &VerificationReport::NotChecked,
            false,
        );

        assert!(
            steps
                .first()
                .is_some_and(|step| step.contains("Fix or clear explicit credential"))
        );
    }

    #[test]
    fn missing_cli_path_is_called_out_before_browser_login() {
        let mut settings = test_settings(DEFAULT_SCOPE);
        settings.service_account_json_path =
            Some("/tmp/does-not-exist-google-search-console-mcp.json".to_string());
        let detection = CredentialDetection {
            gcloud_available: true,
            gcloud_version: Some("Google Cloud SDK 999.0.0".to_string()),
            adc_file: FilePresence {
                path: Some("/tmp/adc.json".to_string()),
                present: false,
            },
            env: EnvPresence {
                google_application_credentials: false,
                google_application_credentials_file_present: false,
                service_account_json_path: false,
                service_account_json_path_file_present: false,
                service_account_json: false,
                oauth_client_secret_json: false,
                oauth_client_secret_json_file_present: false,
                oauth_refresh_token: false,
                cloudsdk_config: false,
            },
        };

        let steps = next_steps(
            &settings,
            &detection,
            &VerificationReport::NotChecked,
            false,
        );

        assert!(
            steps
                .first()
                .is_some_and(|step| step.contains("Fix or clear explicit credential"))
        );
        assert!(!settings_credential_material_detected(&settings));
    }

    #[test]
    fn valid_cli_path_counts_as_credential_material_for_next_steps() {
        let service_account = tempfile::NamedTempFile::new().expect("temp file");
        let mut settings = test_settings(DEFAULT_SCOPE);
        settings.service_account_json_path = Some(
            service_account
                .path()
                .to_str()
                .expect("utf-8 path")
                .to_string(),
        );
        let detection = CredentialDetection {
            gcloud_available: true,
            gcloud_version: Some("Google Cloud SDK 999.0.0".to_string()),
            adc_file: FilePresence {
                path: Some("/tmp/adc.json".to_string()),
                present: false,
            },
            env: EnvPresence {
                google_application_credentials: false,
                google_application_credentials_file_present: false,
                service_account_json_path: false,
                service_account_json_path_file_present: false,
                service_account_json: false,
                oauth_client_secret_json: false,
                oauth_client_secret_json_file_present: false,
                oauth_refresh_token: false,
                cloudsdk_config: false,
            },
        };

        assert!(settings_credential_material_detected(&settings));
        assert!(!explicit_credential_config_needs_repair(
            &settings, &detection
        ));

        let steps = next_steps(
            &settings,
            &detection,
            &VerificationReport::NotChecked,
            false,
        );

        assert!(
            steps
                .first()
                .is_some_and(|step| step.contains("auth status --verify-token"))
        );
    }

    #[test]
    fn post_login_operator_profile_not_ready_without_runtime_write_scope() {
        let mut settings = test_settings(DEFAULT_SCOPE);
        settings.profile = CapabilityProfile::Operator;
        let mut report = AuthReport {
            server: "google-search-console-mcp",
            profile: CapabilityProfile::Operator.as_str().to_string(),
            requested_scope: WRITE_SCOPE.to_string(),
            auth_source: Some("google_default_provider_chain".to_string()),
            auth_source_candidate: Some("google_default_provider_chain".to_string()),
            config_valid: true,
            credential_material_detected: true,
            quota_project_configured: false,
            detected: CredentialDetection {
                gcloud_available: true,
                gcloud_version: Some("Google Cloud SDK 999.0.0".to_string()),
                adc_file: FilePresence {
                    path: Some("/tmp/adc.json".to_string()),
                    present: true,
                },
                env: EnvPresence {
                    google_application_credentials: false,
                    google_application_credentials_file_present: false,
                    service_account_json_path: false,
                    service_account_json_path_file_present: false,
                    service_account_json: false,
                    oauth_client_secret_json: false,
                    oauth_client_secret_json_file_present: false,
                    oauth_refresh_token: false,
                    cloudsdk_config: false,
                },
            },
            verification: VerificationReport::Ok,
            ready: true,
            next_steps: Vec::new(),
        };

        add_post_login_runtime_steps(&settings, true, WRITE_SCOPE, &mut report);

        assert!(!report.ready);
        assert!(verification_ok(&report));
    }

    #[test]
    fn post_login_reports_stale_runtime_scope_after_repaired_login() {
        let settings = test_settings("https://www.googleapis.com/auth/drive");
        let steps = post_login_runtime_steps_with_env(
            &settings,
            false,
            DEFAULT_SCOPE,
            Some("https://www.googleapis.com/auth/drive"),
        );

        assert!(
            steps
                .iter()
                .any(|step| step.contains("GOOGLE_SEARCH_CONSOLE_MCP_SCOPE"))
        );
        assert!(steps.iter().any(|step| step.contains("--scope")));
    }

    #[test]
    fn post_login_marks_report_not_ready_when_runtime_scope_is_stale() {
        let settings = test_settings("https://www.googleapis.com/auth/drive");
        let mut report = AuthReport {
            server: "google-search-console-mcp",
            profile: CapabilityProfile::ReadOnly.as_str().to_string(),
            requested_scope: DEFAULT_SCOPE.to_string(),
            auth_source: Some("google_default_provider_chain".to_string()),
            auth_source_candidate: Some("google_default_provider_chain".to_string()),
            config_valid: true,
            credential_material_detected: true,
            quota_project_configured: false,
            detected: CredentialDetection {
                gcloud_available: true,
                gcloud_version: Some("Google Cloud SDK 999.0.0".to_string()),
                adc_file: FilePresence {
                    path: Some("/tmp/adc.json".to_string()),
                    present: true,
                },
                env: EnvPresence {
                    google_application_credentials: false,
                    google_application_credentials_file_present: false,
                    service_account_json_path: false,
                    service_account_json_path_file_present: false,
                    service_account_json: false,
                    oauth_client_secret_json: false,
                    oauth_client_secret_json_file_present: false,
                    oauth_refresh_token: false,
                    cloudsdk_config: false,
                },
            },
            verification: VerificationReport::Ok,
            ready: true,
            next_steps: Vec::new(),
        };

        add_post_login_runtime_steps_with_env(
            &settings,
            false,
            DEFAULT_SCOPE,
            Some("https://www.googleapis.com/auth/drive"),
            &mut report,
        );

        assert!(!report.ready);
        assert!(
            report
                .next_steps
                .iter()
                .any(|step| step.contains("GOOGLE_SEARCH_CONSOLE_MCP_SCOPE"))
        );
        assert!(
            report
                .next_steps
                .iter()
                .any(|step| step.contains("--scope"))
        );
    }

    #[test]
    fn post_login_reports_stale_env_even_when_explicit_scope_was_used() {
        let custom_scope = "https://www.googleapis.com/auth/webmasters.readonly,https://www.googleapis.com/auth/userinfo.email";
        let settings = test_settings(custom_scope);
        let steps = post_login_runtime_steps_with_env(
            &settings,
            false,
            custom_scope,
            Some("https://www.googleapis.com/auth/drive"),
        );

        assert!(
            steps
                .iter()
                .any(|step| step.contains("GOOGLE_SEARCH_CONSOLE_MCP_SCOPE"))
        );
        assert!(steps.iter().any(|step| step.contains("--scope")));
        assert!(runtime_scope_needs_repair(
            false,
            custom_scope,
            Some("https://www.googleapis.com/auth/drive")
        ));
    }

    fn test_settings(scope: &str) -> Settings {
        Settings {
            profile: CapabilityProfile::ReadOnly,
            scope: scope.to_string(),
            api_base_url: "https://www.googleapis.com/webmasters/v3".to_string(),
            inspection_base_url: "https://searchconsole.googleapis.com/v1".to_string(),
            http_timeout: std::time::Duration::from_secs(1),
            user_agent: "test".to_string(),
            oauth_client_secret_json: None,
            oauth_refresh_token: None,
            service_account_json_path: None,
            service_account_json: None,
            quota_project: None,
            max_row_limit: 25_000,
            print_tools: false,
            print_tool_schema: false,
            command: Some(CliCommand::Serve),
        }
    }
}
