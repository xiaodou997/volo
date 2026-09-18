//! Headless plugin Tool runtime.
//!
//! Background Automation cannot depend on the renderer iframe bridge. This module executes
//! plugin Tool JavaScript in an embedded QuickJS runtime with no DOM, network, filesystem or
//! other host APIs exposed by default.
//!
//! v4 keeps pure-compute Tool support and the low-risk host surface, and adds non-interactive
//! clipboard.readText plus screenCapture as Medium-risk headless APIs. Every host call must be declared by the plugin manifest
//! and is evaluated with the Workflow principal through the same non-interactive background
//! permission path used by BackgroundToolExecutor. Medium/high-risk APIs require a Workflow-scoped
//! Always grant before unattended execution; unsupported host APIs still fail fast.

use std::time::{Duration, Instant};

use rquickjs::{Context, Function, Promise, Runtime};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_notification::NotificationExt;

use crate::api::database::Database;
use crate::core::permission::{enforce_background, PermissionEngine};
use crate::error::{Result, VoloError};
use crate::plugin::manager::{Plugin, PluginState};
use crate::plugin::runner::resolve_run_source;

use super::plugin_tools::lookup_tool;

const HEADLESS_MEMORY_LIMIT: usize = 32 * 1024 * 1024;
const HEADLESS_STACK_LIMIT: usize = 512 * 1024;
const HEADLESS_TOOL_TIMEOUT: Duration = Duration::from_secs(30);

const HEADLESS_SHIM: &str = r#"
(function () {
  var toolCallback = null;

  function call(name, args) {
    var envelopeJson;
    try {
      envelopeJson = __voloHostCall(name, JSON.stringify(args || {}));
    } catch (error) {
      return Promise.reject(error);
    }

    var envelope;
    try {
      envelope = JSON.parse(envelopeJson);
    } catch (error) {
      return Promise.reject(new Error("invalid headless host response: " + error));
    }

    if (!envelope || envelope.ok !== true) {
      return Promise.reject(new Error(
        envelope && envelope.error
          ? String(envelope.error)
          : "headless host call failed: " + name
      ));
    }
    return Promise.resolve(envelope.data);
  }

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
      readText: function () {
        return call("clipboard.readText", {});
      },
      writeText: function (text) {
        return call("clipboard.writeText", { text: text });
      },
      readImage: unsupported("clipboard.readImage"),
      writeImage: unsupported("clipboard.writeImage"),
      readFiles: unsupported("clipboard.readFiles")
    },

    db: {
      put: function (id, data) {
        return call("db.put", { id: id, data: data });
      },
      get: function (id) {
        return call("db.get", { id: id });
      },
      remove: function (id) {
        return call("db.remove", { id: id });
      },
      all: function () {
        return call("db.all", {});
      }
    },

    storage: {
      set: function (key, value) {
        return call("db.put", { id: key, data: value });
      },
      get: function (key) {
        return call("db.get", { id: key }).then(function (doc) {
          return doc && doc.data !== undefined ? doc.data : null;
        });
      },
      remove: function (key) {
        return call("db.remove", { id: key });
      }
    },

    notification: {
      show: function (options) {
        var opts = typeof options === "string" ? { body: options } : options;
        return call("notification.show", { options: opts });
      }
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

    screenCapture: function () {
      return call("screen.capture", {});
    },
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


#[derive(Clone)]
struct HeadlessPluginHost {
    app: AppHandle,
    principal: String,
    plugin_id: String,
    permissions: Vec<String>,
}

impl HeadlessPluginHost {
    fn new(app: AppHandle, principal: &str, plugin: &Plugin) -> Self {
        Self {
            app,
            principal: principal.to_string(),
            plugin_id: plugin.id.clone(),
            permissions: plugin.permissions.clone(),
        }
    }

    fn authorize(&self, capability: &str) -> Result<()> {
        let engine = self.app.state::<PermissionEngine>();
        if !PermissionEngine::declared(&self.permissions, capability, None) {
            engine.audit(&self.principal, capability, None, "deny", None);
            return Err(VoloError::PermissionDenied(format!(
                "Plugin '{}' does not declare permission '{}'",
                self.plugin_id, capability
            )));
        }

        enforce_background(&engine, &self.principal, capability, None)
    }

    fn string_arg<'a>(args: &'a Value, name: &str, method: &str) -> Result<&'a str> {
        args.get(name)
            .and_then(Value::as_str)
            .ok_or_else(|| {
                VoloError::Other(format!(
                    "headless host method '{}' requires string argument '{}'",
                    method, name
                ))
            })
    }

    fn call(&self, method: &str, args: Value) -> Result<Value> {
        match method {
            "clipboard.readText" => {
                self.authorize("clipboard.read")?;
                self.app
                    .clipboard()
                    .read_text()
                    .map(Value::String)
                    .map_err(|error| VoloError::Other(format!("Clipboard read failed: {}", error)))
            }
            "screen.capture" => {
                self.authorize("screen.capture")?;
                crate::api::screen::capture_screen_image().map(Value::String)
            }
            "clipboard.writeText" => {
                self.authorize("clipboard.write")?;
                let text = Self::string_arg(&args, "text", method)?;
                self.app
                    .clipboard()
                    .write_text(text)
                    .map_err(|error| VoloError::Other(format!("Clipboard write failed: {}", error)))?;
                Ok(Value::Null)
            }
            "notification.show" => {
                self.authorize("notification.show")?;
                let options = args
                    .get("options")
                    .and_then(Value::as_object)
                    .ok_or_else(|| {
                        VoloError::Other(
                            "headless host notification.show requires options object".to_string(),
                        )
                    })?;
                let body = options
                    .get("body")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        VoloError::Other(
                            "headless host notification.show requires options.body".to_string(),
                        )
                    })?;
                let title = options
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Volo");

                self.app
                    .notification()
                    .builder()
                    .title(title)
                    .body(body)
                    .show()
                    .map_err(|error| VoloError::Other(format!("Notification failed: {}", error)))?;
                Ok(Value::Null)
            }
            "db.put" => {
                self.authorize("db.write")?;
                let id = Self::string_arg(&args, "id", method)?.to_string();
                let data = args.get("data").cloned().unwrap_or(Value::Null);
                let db = self.app.state::<Database>();
                Ok(serde_json::to_value(db.put_for(&self.plugin_id, id, data)?)?)
            }
            "db.get" => {
                self.authorize("db.read")?;
                let id = Self::string_arg(&args, "id", method)?.to_string();
                let db = self.app.state::<Database>();
                Ok(serde_json::to_value(db.get_for(&self.plugin_id, id)?)?)
            }
            "db.remove" => {
                self.authorize("db.write")?;
                let id = Self::string_arg(&args, "id", method)?.to_string();
                let db = self.app.state::<Database>();
                db.remove_for(&self.plugin_id, id)?;
                Ok(Value::Null)
            }
            "db.all" => {
                self.authorize("db.read")?;
                let db = self.app.state::<Database>();
                Ok(serde_json::to_value(db.all_for(&self.plugin_id)?)?)
            }
            _ => Err(VoloError::Other(format!(
                "headless plugin host API is not supported yet: {}",
                method
            ))),
        }
    }

    fn call_envelope(&self, method: String, args_json: String) -> String {
        let result = serde_json::from_str::<Value>(&args_json)
            .map_err(VoloError::from)
            .and_then(|args| self.call(&method, args));

        match result {
            Ok(data) => serde_json::to_string(&json!({
                "ok": true,
                "data": data
            }))
            .unwrap_or_else(|error| format!(
                "{{\"ok\":false,\"error\":\"serialize host response failed: {}\"}}",
                error
            )),
            Err(error) => serde_json::to_string(&json!({
                "ok": false,
                "error": error.to_string()
            }))
            .unwrap_or_else(|_| {
                "{\"ok\":false,\"error\":\"headless host call failed\"}".to_string()
            }),
        }
    }
}

fn js_error(context: &str, error: impl std::fmt::Display) -> VoloError {
    VoloError::Other(format!("{}: {}", context, error))
}

fn execute_source_sync(
    source: String,
    args: Value,
    timeout: Duration,
    host: Option<HeadlessPluginHost>,
) -> Result<Value> {
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

            let host_fn = Function::new(ctx.clone(), move |method: String, args_json: String| {
                match &host {
                    Some(host) => host.call_envelope(method, args_json),
                    None => serde_json::to_string(&json!({
                        "ok": false,
                        "error": format!(
                            "headless plugin host API is unavailable in pure runtime: {}",
                            method
                        )
                    }))
                    .unwrap_or_else(|_| {
                        "{\"ok\":false,\"error\":\"headless host API unavailable\"}".to_string()
                    }),
                }
            })?
            .with_name("hostCall")?;
            ctx.globals().set("__voloHostCall", host_fn)?;

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
    host: Option<HeadlessPluginHost>,
) -> Result<Value> {
    let source = source.to_string();
    tokio::task::spawn_blocking(move || execute_source_sync(source, args, timeout, host))
        .await
        .map_err(|error| VoloError::Other(format!("headless plugin worker failed: {}", error)))?
}

pub async fn execute_source(source: &str, args: Value) -> Result<Value> {
    execute_source_with_timeout(source, args, HEADLESS_TOOL_TIMEOUT, None).await
}

/// Resolve an LLM plugin-tool name to the installed, enabled plugin and execute its source
/// without a renderer/WebView dependency.
pub async fn execute_plugin_tool(
    app: &AppHandle,
    principal: &str,
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

    let host = HeadlessPluginHost::new(app.clone(), principal, &plugin);
    execute_source_with_timeout(&source, args, HEADLESS_TOOL_TIMEOUT, Some(host)).await
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
    async fn interactive_screen_capture_area_stays_unsupported_in_headless_mode() {
        let source = r#"
            rubick.tool.onInvoke(async function () {
              return await rubick.screenCaptureArea();
            });
        "#;

        let error = execute_source(source, json!({})).await.unwrap_err();
        assert!(error
            .to_string()
            .contains("headless plugin host API is not supported yet: screenCaptureArea"));
    }

    #[tokio::test]
    async fn pure_runtime_does_not_silently_enable_supported_host_calls() {
        let source = r#"
            rubick.tool.onInvoke(async function () {
              return await rubick.clipboard.readText();
            });
        "#;

        let error = execute_source(source, json!({})).await.unwrap_err();
        assert!(error
            .to_string()
            .contains("headless plugin host API is unavailable in pure runtime: clipboard.readText"));
    }

    #[test]
    fn headless_host_method_capabilities_match_existing_plugin_contract() {
        assert!(PermissionEngine::declared(
            &["clipboard.read".to_string()],
            "clipboard.read",
            None
        ));
        assert!(PermissionEngine::declared(
            &["screen.capture".to_string()],
            "screen.capture",
            None
        ));
        assert_eq!(
            PermissionEngine::declared(&["clipboard.write".to_string()], "clipboard.write", None),
            true
        );
        assert_eq!(
            PermissionEngine::declared(&["notification.show".to_string()], "notification.show", None),
            true
        );
        assert_eq!(
            PermissionEngine::declared(&["db.read".to_string()], "db.read", None),
            true
        );
        assert_eq!(
            PermissionEngine::declared(&["db.write".to_string()], "db.write", None),
            true
        );
    }

    #[tokio::test]
    async fn unresolved_promise_fails_instead_of_hanging_scheduler() {
        let source = r#"
            rubick.tool.onInvoke(function () {
              return new Promise(function () {});
            });
        "#;

        let error = execute_source_with_timeout(source, json!({}), Duration::from_millis(30), None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("execute headless plugin tool"));
    }
}
