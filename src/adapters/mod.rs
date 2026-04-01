//! Delivery adapters — transport mechanisms for getting messages to agents.
//!
//! Adapters define HOW a message is delivered (HTTP POST, stdout, file write).
//! Providers define WHERE (which platform). Providers USE adapters internally.
//!
//! A provider like OpenClaw uses the HTTP adapter to POST to /hooks/agent.
//! A provider like Claude Code uses the File adapter to write to ~/.claude/messages/.
//! A user can also use adapters directly via --platform webhook/stdout/file.
//!
//! ```text
//! adapters/
//! ├── mod.rs         ← Barrel exports
//! ├── adapter.rs     ← Adapter trait (SSOT interface)
//! ├── http.rs        ← HTTP POST to any URL (used by webhook, openclaw)
//! ├── stdout.rs      ← JSON to stdout (pipe-friendly)
//! └── file.rs        ← JSON files to directory (inotify-friendly)
//! ```

pub mod adapter;
pub mod http;
pub mod stdout;
pub mod file;

pub use adapter::{Adapter, AdapterConfig};
pub use http::HttpAdapter;
pub use stdout::StdoutAdapter;
pub use file::FileAdapter;
