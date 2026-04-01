# Changelog

## [0.4.0] — 2026-04-01

### Added
- **Two-layer architecture**: adapters (transport) + providers (platform)
- `src/adapters/` — reusable transport mechanisms:
  - `adapter.rs` — Adapter trait (SSOT) + TransportResult
  - `http.rs` — HTTP POST adapter (used by OpenClaw, Webhook providers)
  - `stdout.rs` — JSON to stdout adapter
  - `file.rs` — JSON to directory adapter
- Providers now COMPOSE adapters (no duplicated HTTP/file logic)

### Changed
- OpenClawProvider internally uses HttpAdapter
- WebhookProvider wraps HttpAdapter
- StdoutProvider wraps StdoutAdapter
- FileProvider wraps FileAdapter

## [0.3.0] — 2026-04-01

### Added
- **Provider architecture** — SSOT `Provider` trait for agent platform integrations
- `src/providers/` — one file per platform:
  - `provider.rs` — Provider trait + Message + DeliveryResult
  - `detect.rs` — Auto-detection scans machine for installed platforms
  - `openclaw.rs` — OpenClaw /hooks/agent (fully implemented)
  - `claude_code.rs` — Claude Code (file-based delivery)
  - `codex.rs` — OpenAI Codex CLI (stub)
  - `gemini.rs` — Google Gemini CLI (stub)
  - `copilot.rs` — GitHub Copilot (stub)
  - `opencode.rs` — OpenCode (stub)
  - `generic.rs` — Webhook, Stdout, File (universal fallbacks)
- `signaldock providers` command — lists all available providers
- PROVIDER_NAMES registry in mod.rs

### Changed
- **BREAKING**: Replaced v0.2.0 `adapters/` with `providers/`. The old PlatformAdapter
  trait evolved into the Provider trait (adds detect(), info(), status_line()).

### Migration from adapters → providers
- Old `PlatformAdapter.name()` → New `Provider.info().name`
- Old `PlatformAdapter.deliver(from, content, id, conv)` → New `Provider.deliver(&Message)`
- Old `PlatformAdapter.is_healthy()` → Same, still on Provider trait
- Old detect logic (in mod.rs) → New detect.rs with per-provider detect() methods
- Old factory (in mod.rs) → New factory in detect.rs create_provider()

## [0.2.0] — 2026-04-01

### Added
- Modular `src/adapters/` directory (first refactor from monolithic adapter.rs):
  - `base.rs` — PlatformAdapter trait, Message struct, DeliveryResult enum
  - `openclaw.rs`, `webhook.rs`, `stdout.rs`, `file_output.rs` — concrete adapters
  - `detect.rs` — platform auto-detection (SRP)
  - `factory.rs` — adapter creation factory (SRP)
- CHANGELOG.md

### Changed
- **BREAKING**: Replaced monolithic `adapter.rs` with modular `adapters/` directory

## [0.1.1] — 2026-04-01

### Fixed
- Switched from SSE-primary to poll-primary receiver (SSE broken server-side)
- Changed logging from tracing (silent) to eprintln
- Verified OpenClaw hooks/agent end-to-end

### Removed
- reqwest-eventsource, futures-util dependencies

## [0.1.0] — 2026-04-01

### Added
- Initial release: CLI, poll receiver, OpenClaw/webhook/stdout adapters
- Config persistence, systemd service generation, message dedup
- 6MB static binary, Rust 1.94
