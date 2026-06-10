//! Tier 1 plugin runtime: dispatching contributed actions and commands to
//! the plugin's JSON-RPC worker.
//!
//! This module is the single entry point every surface (TUI keybinds, CLI
//! grafted commands, web action routes) calls to run plugin code. The worker
//! host (spawn, supervise, capability middleware) lands in P6; until then
//! invocations fail with a clear "runtime not available" error rather than
//! pretending to work.

use anyhow::{anyhow, Result};
use serde_json::Value;

/// Invoke a plugin-contributed action or command over the plugin's worker.
pub fn invoke_action(plugin_id: &str, rpc_method: &str, params: Value) -> Result<Value> {
    let _ = params;
    Err(anyhow!(
        "plugin {plugin_id} declares {rpc_method}, but the Tier 1 worker runtime is not running \
         (worker host lands with the plugin-host phase)"
    ))
}
