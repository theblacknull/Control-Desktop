use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::sandbox_action::SandboxAction;
use crate::sandbox::exec_passthrough::ExecPassthrough;
use crate::sandbox::provider_dirs::{SANDBOXES_DIR, resolve};
use crate::sandbox::provider_protocol::{exec_outcome, parse_reply};
use crate::sandbox::provider_reply::ProviderReply;
use crate::sandbox::provider_run::{ProviderCall, success_value};
use crate::sandbox::provider_settings::parse_settings;
use crate::sandbox::provider_spec::ProviderSpec;
use crate::sandbox::provider_table::find_provider;
use crate::sandbox::sandbox_name::validate_sandbox_name;
use crate::sandbox::sandbox_record::SandboxRecord;
use crate::sandbox::sandbox_state::SandboxState;
use crate::sandbox::sandbox_store;
use crate::storage::state_root::resolve_configured_state_root;
use crate::{AdapterError, AppError, DeliverySemantics, ErrorCode, PlatformAdapter};

const CREATE_TIMEOUT: Duration = Duration::from_secs(60);
const START_TIMEOUT: Duration = Duration::from_secs(60);
const STOP_TIMEOUT: Duration = Duration::from_secs(30);
const STATUS_TIMEOUT: Duration = Duration::from_secs(30);
const REMOVE_TIMEOUT: Duration = Duration::from_secs(30);
const EXEC_TIMEOUT: Duration = Duration::from_secs(300);

pub fn execute(action: SandboxAction, adapter: &dyn PlatformAdapter) -> Result<Value, AppError> {
    let root = resolve_configured_state_root()?;
    match action {
        SandboxAction::Create {
            name,
            provider,
            set,
        } => create(&root, &name, &provider, &set, adapter),
        SandboxAction::Start { name } => verb_value(&root, &name, "start", START_TIMEOUT, adapter),
        SandboxAction::Stop { name } => verb_value(&root, &name, "stop", STOP_TIMEOUT, adapter),
        SandboxAction::Status { name } => {
            verb_value(&root, &name, "status", STATUS_TIMEOUT, adapter)
        }
        SandboxAction::List => list(&root, adapter),
        SandboxAction::Rm { name } => remove(&root, &name, adapter),
        SandboxAction::Exec { name, .. } => Err(exec_without_envelope(&name)),
    }
}

pub fn execute_exec(
    name: String,
    args: Vec<String>,
    adapter: &dyn PlatformAdapter,
) -> Result<ExecPassthrough, AppError> {
    let root = resolve_configured_state_root()?;
    let (spec, provider_dir, sandboxes_root) = locate(&root, &name)?;
    let call = ProviderCall::new(spec, &provider_dir, &sandboxes_root, Some(&name), &[]);
    let mut guest_args = vec!["exec".to_string(), name.clone(), "--".to_string()];
    guest_args.extend(args);
    let output = call.run(adapter, guest_args, EXEC_TIMEOUT)?;
    exec_outcome(spec.name, &output)
}

fn create(
    root: &Path,
    name: &str,
    provider: &str,
    set: &[String],
    adapter: &dyn PlatformAdapter,
) -> Result<Value, AppError> {
    validate_sandbox_name(name)?;
    let spec = find_provider(provider)?;
    let settings = parse_settings(set)?;
    let (provider_dir, sandboxes_root) = resolve(root, spec)?;
    sandbox_store::create(
        &sandboxes_root,
        &SandboxRecord {
            name: name.to_string(),
            provider: provider.to_string(),
            created_at: unix_seconds(),
        },
    )?;
    let call = ProviderCall::new(spec, &provider_dir, &sandboxes_root, Some(name), &settings);
    let outcome = call
        .run(adapter, verb_args(name, "create"), CREATE_TIMEOUT)
        .and_then(|output| parse_reply(spec.name, Some(name), true, &output))
        .and_then(|reply| {
            if reply.state == SandboxState::Unavailable {
                Err(unavailable_refusal(reply.message))
            } else {
                Ok(reply)
            }
        });
    match outcome {
        Ok(reply) => Ok(success_value(name, Some(spec.name), reply)),
        Err(error) => {
            discard_untouched_record(&sandboxes_root, name);
            Err(error)
        }
    }
}

fn discard_untouched_record(sandboxes_root: &Path, name: &str) {
    let dir = sandboxes_root.join(name);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let untouched = entries
        .filter_map(Result::ok)
        .all(|entry| entry.file_name() == sandbox_store::RECORD_FILE);
    if untouched {
        let _ = sandbox_store::remove(sandboxes_root, name);
    }
}

fn verb_value(
    root: &Path,
    name: &str,
    verb: &str,
    timeout: Duration,
    adapter: &dyn PlatformAdapter,
) -> Result<Value, AppError> {
    let (spec, provider_dir, sandboxes_root) = locate(root, name)?;
    let call = ProviderCall::new(spec, &provider_dir, &sandboxes_root, Some(name), &[]);
    let output = call.run(adapter, verb_args(name, verb), timeout)?;
    let reply = parse_reply(spec.name, Some(name), is_mutating(verb), &output)?;
    Ok(success_value(name, Some(spec.name), reply))
}

fn remove(root: &Path, name: &str, adapter: &dyn PlatformAdapter) -> Result<Value, AppError> {
    let (spec, provider_dir, sandboxes_root) = locate(root, name)?;
    let call = ProviderCall::new(spec, &provider_dir, &sandboxes_root, Some(name), &[]);
    let status = call.run(adapter, verb_args(name, "status"), STATUS_TIMEOUT)?;
    let reply = parse_reply(spec.name, Some(name), false, &status)?;
    let ProviderReply { state, .. } = reply;
    if state == SandboxState::Running {
        return Err(running_refusal(name));
    }
    let output = call.run(adapter, verb_args(name, "rm"), REMOVE_TIMEOUT)?;
    let reply = parse_reply(spec.name, Some(name), true, &output)?;
    sandbox_store::remove(&sandboxes_root, name)?;
    Ok(success_value(name, Some(spec.name), reply))
}

fn list(root: &Path, adapter: &dyn PlatformAdapter) -> Result<Value, AppError> {
    let records = sandbox_store::list(&root.join(SANDBOXES_DIR))?;
    let mut sandboxes = Vec::with_capacity(records.len());
    for record in &records {
        sandboxes.push(record_entry(root, record, adapter));
    }
    Ok(json!({ "sandboxes": sandboxes }))
}

fn record_entry(root: &Path, record: &SandboxRecord, adapter: &dyn PlatformAdapter) -> Value {
    match status_reply(root, record, adapter) {
        Ok(reply) => success_value(&record.name, Some(&record.provider), reply),
        Err(error) => broken_entry(record, &error),
    }
}

fn status_reply(
    root: &Path,
    record: &SandboxRecord,
    adapter: &dyn PlatformAdapter,
) -> Result<ProviderReply, AppError> {
    let spec = find_provider(&record.provider)?;
    let (provider_dir, sandboxes_root) = resolve(root, spec)?;
    let call = ProviderCall::new(
        spec,
        &provider_dir,
        &sandboxes_root,
        Some(&record.name),
        &[],
    );
    let output = call.run(adapter, verb_args(&record.name, "status"), STATUS_TIMEOUT)?;
    parse_reply(spec.name, Some(&record.name), false, &output)
}

fn locate(root: &Path, name: &str) -> Result<(&'static ProviderSpec, PathBuf, PathBuf), AppError> {
    let record = sandbox_store::load(&root.join(SANDBOXES_DIR), name)?;
    let spec = find_provider(&record.provider)?;
    let (provider_dir, sandboxes_root) = resolve(root, spec)?;
    Ok((spec, provider_dir, sandboxes_root))
}

fn verb_args(name: &str, verb: &str) -> Vec<String> {
    vec![verb.to_string(), name.to_string()]
}

fn is_mutating(verb: &str) -> bool {
    matches!(verb, "create" | "start" | "stop" | "rm")
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn running_refusal(name: &str) -> AppError {
    AppError::Adapter(
        AdapterError::new(
            ErrorCode::InvalidArgs,
            format!("Sandbox {name} is running, so it was not removed."),
        )
        .with_suggestion(format!("Run `agent-desktop sandbox stop {name}` first."))
        .with_disposition(DeliverySemantics::not_delivered()),
    )
}

fn broken_entry(record: &SandboxRecord, error: &AppError) -> Value {
    json!({
        "name": &record.name,
        "provider": &record.provider,
        "state": SandboxState::Broken,
        "message": error.to_string(),
        "next": format!("Run `agent-desktop sandbox rm {}` to remove it.", record.name),
    })
}

fn unavailable_refusal(message: String) -> AppError {
    AppError::Adapter(
        AdapterError::new(ErrorCode::PlatformNotSupported, message)
            .with_suggestion("Run `agent-desktop install` to see what each provider needs.")
            .with_disposition(DeliverySemantics::not_delivered()),
    )
}

fn exec_without_envelope(name: &str) -> AppError {
    AppError::Adapter(
        AdapterError::new(
            ErrorCode::ActionNotSupported,
            "`sandbox exec` prints the sandboxed program's own output, so it has no data envelope of its own.",
        )
        .with_suggestion(format!(
            "Run `agent-desktop sandbox exec {name} -- snapshot --app Finder` to run a command in the sandbox."
        ))
        .with_disposition(DeliverySemantics::not_delivered()),
    )
}

#[cfg(test)]
#[path = "sandbox_tests.rs"]
mod tests;
