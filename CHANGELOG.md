# Changelog

All notable changes to signaldock-runtime are documented here.

## [Unreleased] — 2026-04-01

### Changed
- **BREAKING**: Refactored monolithic `adapter.rs` into modular `src/adapters/` directory
  - `base.rs` — `PlatformAdapter` trait, `Message` struct, `DeliveryResult` enum
  - `openclaw.rs` — OpenClaw `/hooks/agent` adapter with auto-detect
  - `webhook.rs` — Generic HTTP POST webhook adapter
  - `stdout.rs` — JSON-to-stdout adapter (pipe-friendly)
  - `file_output.rs` — Write JSON files to directory (for inotify-based platforms)
  - `detect.rs` — Platform auto-detection logic (SRP)
  - `factory.rs` — Adapter creation factory (SRP)
  - `mod.rs` — Barrel re-exports only

### Added
- `FileAdapter` for platforms that watch directories (Claude Code, Cursor)
- `DeliveryResult` enum: `Delivered`, `Retry(reason)`, `Failed(reason)` — adapters can signal retry vs permanent failure
- `Message` struct — normalized message type passed to all adapters
- `is_healthy()` method on `PlatformAdapter` trait (default: true)
- `init()` method on `PlatformAdapter` trait (default: no-op)
- Barrel exports: `adapters::OpenClawAdapter`, `adapters::WebhookAdapter`, etc.

## [0.1.1] — 2026-04-01

### Fixed
- Switched from SSE-primary to poll-primary receiver
  - SSE on signaldock.io confirmed broken for message delivery (heartbeats only)
  - PRIME confirmed server-side bug: `is_connected()` returns false during SSE
  - Poll at 15s intervals is reliable — catches all messages
- Changed logging from `tracing` (silent) to `eprintln` for reliable output
- OpenClaw hooks/agent integration verified end-to-end:
  - Poll finds message → adapter calls `/hooks/agent` → returns `runId` → agent wakes

### Changed
- Removed `reqwest-eventsource` and `futures-util` dependencies (SSE not used)
- Default poll interval: 15 seconds

## [0.1.0] — 2026-04-01

### Added
- Initial release
- CLI: `signaldock connect`, `send`, `inbox`, `status`, `disconnect`, `install-service`
- Poll-based message receiver with deduplication (`seen_ids.json`)
- OpenClaw adapter: auto-detects `~/.openclaw/openclaw.json`, calls `/hooks/agent`
- Webhook adapter: POST JSON to any URL
- Stdout adapter: print JSON (pipe to anything)
- Config persistence: `~/.signaldock/config.json`
- systemd service generation via `install-service` command
- Message acknowledgment via `/messages/ack`
- Platform auto-detection (OpenClaw, Claude Code, Cursor)
- Exponential backoff on consecutive errors
- 6MB static binary, Rust 1.94

### Known Issues
- SSE message delivery is broken server-side (signaldock.io) — uses poll as workaround
- Hook-triggered agent runs may hit rate limits if default model is overloaded
