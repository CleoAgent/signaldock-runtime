//! Auto-detection and factory for providers.
//!
//! Scans the machine for installed agent platforms and returns
//! the first match. Order is defined by PROVIDER_NAMES in mod.rs.

use anyhow::{Context, Result};
use crate::config::Config;
use super::provider::Provider;
use super::*;

/// Auto-detect which provider is available on this machine.
/// Returns the platform name string for config.
pub fn detect_provider() -> String {
    // Try each provider's detect() in priority order
    if OpenClawProvider::detect().is_some() { return "openclaw".into(); }
    if ClaudeCodeProvider::detect().is_some() { return "claude-code".into(); }
    if CodexProvider::detect().is_some() { return "codex".into(); }
    if GeminiProvider::detect().is_some() { return "gemini".into(); }
    if CopilotProvider::detect().is_some() { return "copilot".into(); }
    if OpenCodeProvider::detect().is_some() { return "opencode".into(); }

    eprintln!("[signaldock] No agent platform detected — using stdout");
    "stdout".into()
}

/// Create a provider instance from config.
pub fn create_provider(config: &Config) -> Result<Box<dyn Provider>> {
    match config.platform.as_str() {
        "openclaw" => {
            OpenClawProvider::detect()
                .or_else(|| Some(Box::new(OpenClawProvider::new_default())))
                .context("OpenClaw provider failed")
        }
        "claude-code" => {
            ClaudeCodeProvider::detect()
                .context("Claude Code not found — is it installed?")
        }
        "codex" => {
            CodexProvider::detect()
                .context("Codex CLI not found — is it installed?")
        }
        "gemini" => {
            GeminiProvider::detect()
                .context("Gemini CLI not found — is it installed?")
        }
        "copilot" => {
            CopilotProvider::detect()
                .context("Copilot not found — is it installed?")
        }
        "opencode" => {
            OpenCodeProvider::detect()
                .context("OpenCode not found — is it installed?")
        }
        "webhook" => {
            let url = config.webhook_url.as_ref()
                .context("--webhook URL required")?;
            Ok(Box::new(WebhookProvider::new(url.clone())))
        }
        "file" => {
            let dir = config.file_output_dir.clone()
                .unwrap_or_else(|| {
                    dirs::home_dir().unwrap_or_default()
                        .join(".signaldock/messages")
                        .to_string_lossy().to_string()
                });
            Ok(Box::new(FileProvider::new(dir)))
        }
        "stdout" | _ => {
            Ok(Box::new(StdoutProvider))
        }
    }
}
