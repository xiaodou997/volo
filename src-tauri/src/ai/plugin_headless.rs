//! Headless plugin Tool runtime.
//!
//! Background Automation cannot depend on the renderer iframe bridge. This module executes
//! plugin Tool JavaScript in an embedded QuickJS runtime with no DOM, network, filesystem or
//! other host APIs exposed by default.
//!
//! v1 deliberately supports pure-compute Tool scripts only. Existing `rubick.*` host APIs
//! are present as explicit rejections so a plugin fails fast instead of silently bypassing the
//! background permission model. Host APIs can be added later through the same non-interactive
//! permission path used by BackgroundToolExecutor.

use std::time::{Duration, Instant};

use rquickjs::{Context, Function, Promise, Runtime};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::error::{Result, VoloError};
use crate::plugin::manager::PluginState;
use crate::plugin::runner::resolve_run_source;

use super::plugin_tools::lookup_tool;

const HEADLESS_MEMORY_LIMIT: usize = 32 * 1024 * 1024;
const HEADLESS_STACK_LIMIT: usize = 512 * 1024;
const HEADLESS_TOOL_TIMEOUT: Duration = Duration::from_secs(30);

const HEADLESS_SHIM: &str = r#"
(function () {
  var toolCallback = null;

  function unsupported(name) {
    return function () {
      return Promise.reject(new Error(
        "headless plugin host API is not supported yet: " + name
      ));
    };
  }

  globalThis.console = globalThis.console || {
    log: function () {},
    info: function () {},
    warn: function () {},
    error: function () {}
  };

  globalThis.crypto = Object.freeze({
    randomUUID: function () { return __voloRandomUuid(); }
  });

  globalThis.rubick = {
    __isVoloBridge: true,
    tool: {
      onInvoke: function (cb) {
        if (typeof cb !== "function") {
          throw new Error("tool.onInvoke requires a function");
        }
        toolCallback = cb;
      }
    },

    clipboard: {
      readText: unsupported("clipboard.readText"),
      writeText: unsupported("clipboard.writeText"),
      readImage: unsupported("clipboard.readImage"),
      writeImage: unsupported("clipboard.writeImage"),
      readFiles: unsupported("clipboard.readFiles")
    },

    db: {
      put: unsupported("db.put"),
      get: unsupported("db.get"),
      remove: unsupported("db.remove"),
      all: unsupported("db.all")
    },

    storage: {
      set: unsupported("storage.set"),
      get: unsupported("storage.get"),
      remove: unsupported("storage.remove")
    },

    notification: {
      show: unsupported("notification.show")
    },

    shell: {
      open: unsupported("shell.open"),
      openPath: unsupported("shell.openPath")
    },

    fs: {
      read: unsupported("fs.read"),
      readBinary: unsupported("fs.readBinary"),
      write: unsupported("fs.write"),
      writeBinary: unsupported("fs.writeBinary"),
      exists: unsupported("fs.exists"),
      mkdir: unsupported("fs.mkdir"),
      remove: unsupported("fs.remove"),
      list: unsupported("fs.list"),
      pickFile: unsupported("fs.pickFile"),
      pickFiles: unsupported("fs.pickFiles"),
      pickFolder: unsupported("fs.pickFolder")
    },

    screenCapture: unsupported("screenCapture"),
    screenCaptureArea: unsupported("screenCaptureArea"),

    window: {
      hide: unsupported("window.hide"),
      show: unsupported("window.show"),
      setSize: unsupported("window.setSize")
    },

    subInput: {
      show: unsupported("subInput.show"),
      hide: unsupported("subInput.hide"),
      setValue: unsupported("subInput.setValue"),
      onChange: unsupported("subInput.onChange")
    },

    system: {
      platform: "headless",
      darkMode: false,
      version: "headless"
    }
  };

  globalThis.__voloInvoke = async function (inputJson) {
    try {
      if (typeof toolCallback !== "function") {
        throw new Error("tool.onInvoke callback is not registered");
      }

      var input = {};
      if (typeof inputJson === "string" && inputJson) {
        input = JSON.parse(inputJson);
      }

      var result = await toolCallback(input);
      var normalized = result === undefined ? null : result;
      var dataJson = JSON.stringify(normalized);
      if (dataJson === undefined) {
        throw new Error("tool result is not JSON serializable");
      }

      return JSON.stringify({
        ok: true,
        data: JSON.parse(dataJson)
      });
    } catch (error) {
      var message = error && error.message
        ? String(error.message)
        : String(error);
      return JSON.stringify({
        ok: false,
        error: message
      });
    }
  };
})();
"#;

fn js_error(context: &str, error: impl std::fmt::Display) -> VoloError {
    VoloError::Other(format!("{}: {}", context, error))
}

fn execute_source_sync(source: String, args: Value, timeout: Duration) -> Result<Value> {
    let runtime =
        Runtime::new().map_err(|error| js_error("create headless QuickJS runtime", error))?;
    runtime.set_memory_limit(HEADLESS_MEMORY_LIMIT);
    runtime.set_max_stack_size(HEADLESS_STACK_LIMIT);

    let deadline = Instant::now() + timeout;
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));

    let context =
        Context::full(&runtime).map_err(|error| js_error("create headless QuickJS context", error))?;
    let input_json = serde_json::to_string(&args)?;

    let envelope_json = context
        .with(|ctx| -> rquickjs::Result<String> {
            let uuid_fn = Function::new(ctx.clone(), || uuid::Uuid::new_v4().to_string())?
                .with_name("randomUUID")?;
            ctx.globals().set("__voloRandomUuid", uuid_fn)?;

            ctx.eval::<(), _>(HEADLESS_SHIM)?;
            ctx.eval::<(), _>(source)?;

            let invoke: Function = ctx.globals().get("__voloInvoke")?;
            let promise: Promise = invoke.call((input_json,))?;
            promise.finish::<String>()
        })
        .map_err(|error| js_error("execute headless plugin tool", error))?;

    let envelope: Value = serde_json::from_str(&envelope_json)?;
    if envelope.get("ok").and_then(Value::as_bool) == Some(true) {
        return Ok(envelope.get("data").cloned().unwrap_or(Value::Null));
    }

    Err(VoloError::Other(
        envelope
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("headless plugin tool failed")
            .to_string(),
    ))
}

async fn execute_source_with_timeout(
    source: &str,
    args: Value,
    timeout: Duration,
) -> Result<Value> {
    let source = source.to_string();
    tokio::task::spawn_blocking(move || execute_source_sync(source, args, timeout))
        .await
        .map_err(|error| VoloError::Other(format!("headless plugin worker failed: {}", error)))?
}

pub async fn execute_source(source: &str, args: Value) -> Result<Value> {
    execute_source_with_timeout(source, args, HEADLESS_TOOL_TIMEOUT).await
}

/// Resolve an LLM plugin-tool name to the installed, enabled plugin and execute its source
/// without a renderer/WebView dependency.
pub async fn execute_plugin_tool(
    app: &AppHandle,
    _principal: &str,
    llm_name: &str,
    args: Value,
) -> Result<Value> {
    let plugins = app.state::<PluginState>();
    let (plugin_id, tool_id) = lookup_tool(&plugins, llm_name)
        .ok_or_else(|| VoloError::NotFound(format!("plugin tool: {}", llm_name)))?;

    let plugin = plugins
        .get_plugin(&plugin_id)
        .ok_or_else(|| VoloError::NotFound(format!("plugin: {}", plugin_id)))?;
    let tool = plugin
        .contributes
        .tools
        .iter()
        .find(|tool| tool.id == tool_id)
        .ok_or_else(|| VoloError::NotFound(format!("plugin tool: {}/{}", plugin_id, tool_id)))?;

    let source_path = resolve_run_source(&plugin, &tool.run)?;
    let source = std::fs::read_to_string(&source_path).map_err(|error| {
        VoloError::Other(format!(
            "failed to read plugin tool source '{}': {}",
            source_path.display(),
            error
        ))
    })?;

    execute_source(&source, args).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pure_tool_executes_without_renderer() {
        let source = r#"
            rubick.tool.onInvoke(function (input) {
              return {
                total: input.a + input.b,
                nested: { ok: true }
              };
            });
        "#;

        let result = execute_source(source, json!({ "a": 2, "b": 3 }))
            .await
            .unwrap();
        assert_eq!(result, json!({ "total": 5, "nested": { "ok": true } }));
    }

    #[tokio::test]
    async fn async_pure_tool_result_is_awaited() {
        let source = r#"
            rubick.tool.onInvoke(async function (input) {
              return Promise.resolve({ value: input.value + 1 });
            });
        "#;

        let result = execute_source(source, json!({ "value": 8 }))
            .await
            .unwrap();
        assert_eq!(result, json!({ "value": 9 }));
    }

    #[tokio::test]
    async fn crypto_random_uuid_is_available_for_existing_uuid_tool_style() {
        let source = r#"
            rubick.tool.onInvoke(function () {
              return { first: crypto.randomUUID(), second: crypto.randomUUID() };
            });
        "#;

        let result = execute_source(source, json!({})).await.unwrap();
        let first = result["first"].as_str().unwrap();
        let second = result["second"].as_str().unwrap();
        assert_eq!(first.len(), 36);
        assert_eq!(second.len(), 36);
        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn host_api_fails_fast_instead_of_using_renderer_or_bypassing_permissions() {
        let source = r#"
            rubick.tool.onInvoke(async function () {
              return await rubick.clipboard.readText();
            });
        "#;

        let error = execute_source(source, json!({})).await.unwrap_err();
        assert!(error
            .to_string()
            .contains("headless plugin host API is not supported yet: clipboard.readText"));
    }

    #[tokio::test]
    async fn unresolved_promise_fails_instead_of_hanging_scheduler() {
        let source = r#"
            rubick.tool.onInvoke(function () {
              return new Promise(function () {});
            });
        "#;

        let error = execute_source_with_timeout(source, json!({}), Duration::from_millis(30))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("execute headless plugin tool"));
    }
}
