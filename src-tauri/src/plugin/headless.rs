//! Native headless JavaScript runtime for plugin tools.
//!
//! The renderer plugin bridge remains the compatibility runtime. Tools must opt in with
//! `runtime: "headless"` before Automation may execute them here.
//!
//! Security boundary for the first slice:
//! - a fresh QuickJS runtime/context per invocation;
//! - strict memory/stack limits plus an interrupt deadline;
//! - no filesystem/network/shell/clipboard bridge is installed;
//! - only standard ECMAScript globals and `crypto.randomUUID()` are provided;
//! - input/output must round-trip through JSON.

use std::time::{Duration, Instant};

use rquickjs::function::Func;
use rquickjs::{Context, Function, Object, Promise, Runtime, Value as JsValue};
use serde_json::Value;

use crate::error::{Result, VoloError};
use crate::plugin::manager::{Plugin, ToolManifestSpec, ToolRuntime};
use crate::plugin::runner::resolve_run_source;

const HEADLESS_TOOL_TIMEOUT: Duration = Duration::from_secs(2);
const HEADLESS_TOOL_MEMORY_LIMIT: usize = 16 * 1024 * 1024;
const HEADLESS_TOOL_STACK_LIMIT: usize = 512 * 1024;

const TOOL_BOOTSTRAP: &str = r#"
globalThis.__voloToolCallback = null;
globalThis.rubick = Object.freeze({
  tool: Object.freeze({
    onInvoke: function (callback) {
      if (typeof callback !== 'function') {
        throw new TypeError('rubick.tool.onInvoke expects a function');
      }
      globalThis.__voloToolCallback = callback;
    }
  })
});
"#;

fn js_error(error: rquickjs::Error, deadline: Instant) -> VoloError {
    if Instant::now() >= deadline {
        return VoloError::Plugin(format!(
            "headless plugin tool execution timed out after {} ms",
            HEADLESS_TOOL_TIMEOUT.as_millis()
        ));
    }
    VoloError::Plugin(format!("headless plugin JavaScript error: {error}"))
}

fn execute_source_with_limits(
    source: &str,
    input: Value,
    timeout: Duration,
    memory_limit: usize,
) -> Result<Value> {
    let runtime = Runtime::new()
        .map_err(|error| VoloError::Plugin(format!("failed to create QuickJS runtime: {error}")))?;
    runtime.set_memory_limit(memory_limit);
    runtime.set_max_stack_size(HEADLESS_TOOL_STACK_LIMIT);

    let deadline = Instant::now() + timeout;
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));

    let context = Context::full(&runtime)
        .map_err(|error| VoloError::Plugin(format!("failed to create QuickJS context: {error}")))?;

    context.with(|ctx| -> Result<Value> {
        let globals = ctx.globals();

        // QuickJS deliberately does not provide browser Web APIs. UUID generation is useful for
        // pure utility tools and does not grant access to host data, so expose only this tiny API.
        let crypto = Object::new(ctx.clone()).map_err(|error| js_error(error, deadline))?;
        crypto
            .set("randomUUID", Func::from(|| uuid::Uuid::new_v4().to_string()))
            .map_err(|error| js_error(error, deadline))?;
        globals
            .set("crypto", crypto)
            .map_err(|error| js_error(error, deadline))?;

        ctx.eval::<(), _>(TOOL_BOOTSTRAP)
            .map_err(|error| js_error(error, deadline))?;
        ctx.eval::<(), _>(source)
            .map_err(|error| js_error(error, deadline))?;

        let callback: Function<'_> = globals.get("__voloToolCallback").map_err(|_| {
            VoloError::Plugin(
                "headless plugin tool did not register rubick.tool.onInvoke callback".to_string(),
            )
        })?;

        let js_input = rquickjs_serde::to_value(ctx.clone(), input)
            .map_err(|error| VoloError::Plugin(format!("invalid plugin tool input: {error}")))?;
        let mut output: JsValue<'_> = callback
            .call((js_input,))
            .map_err(|error| js_error(error, deadline))?;

        if output.is_promise() {
            output = Promise::from_value(output)
                .map_err(|error| js_error(error, deadline))?
                .finish::<JsValue<'_>>()
                .map_err(|error| js_error(error, deadline))?;
        }

        // Renderer runtime turns undefined into null before crossing postMessage. Keep the two
        // runtimes behaviorally aligned.
        if output.is_undefined() {
            return Ok(Value::Null);
        }

        rquickjs_serde::from_value_strict(output).map_err(|error| {
            VoloError::Plugin(format!(
                "headless plugin tool result is not JSON serializable: {error}"
            ))
        })
    })
}

/// Execute one explicitly headless plugin tool without using the renderer bridge.
pub async fn execute_headless_tool(
    plugin: Plugin,
    tool: ToolManifestSpec,
    input: Value,
) -> Result<Value> {
    if tool.runtime != ToolRuntime::Headless {
        return Err(VoloError::Plugin(format!(
            "plugin tool {}/{} uses renderer runtime and cannot run headlessly",
            plugin.id, tool.id
        )));
    }

    let source_path = resolve_run_source(&plugin, &tool.run)?;
    let source = std::fs::read_to_string(&source_path).map_err(|error| {
        VoloError::Plugin(format!(
            "failed to read headless plugin tool source {}: {}",
            source_path.display(),
            error
        ))
    })?;

    tokio::task::spawn_blocking(move || {
        execute_source_with_limits(
            &source,
            input,
            HEADLESS_TOOL_TIMEOUT,
            HEADLESS_TOOL_MEMORY_LIMIT,
        )
    })
    .await
    .map_err(|error| VoloError::Plugin(format!("headless plugin runtime task failed: {error}")))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn run(source: &str, input: Value) -> Result<Value> {
        execute_source_with_limits(source, input, Duration::from_millis(250), 8 * 1024 * 1024)
    }

    fn plugin_and_tool(runtime: ToolRuntime) -> (Plugin, ToolManifestSpec) {
        let tool = ToolManifestSpec {
            id: "tool".to_string(),
            name: "Tool".to_string(),
            description: None,
            parameters: json!({ "type": "object", "properties": {} }),
            run: "tool.js".to_string(),
            icon: None,
            runtime,
        };
        let plugin = Plugin {
            id: "plugin".to_string(),
            name: "Plugin".to_string(),
            version: "1.0.0".to_string(),
            main: "index.html".to_string(),
            path: PathBuf::new(),
            features: vec![],
            permissions: vec![],
            description: None,
            icon: None,
            contributes: crate::plugin::manager::Contributes {
                commands: vec![],
                tools: vec![tool.clone()],
            },
        };
        (plugin, tool)
    }

    #[test]
    fn executes_sync_tool_with_json_round_trip() {
        let output = run(
            r#"
            rubick.tool.onInvoke(function (input) {
              return { doubled: input.value * 2, nested: input.nested };
            });
            "#,
            json!({ "value": 4, "nested": { "ok": true } }),
        )
        .unwrap();

        assert_eq!(output, json!({ "doubled": 8, "nested": { "ok": true } }));
    }

    #[test]
    fn executes_resolved_promise_result() {
        let output = run(
            r#"
            rubick.tool.onInvoke(function (input) {
              return Promise.resolve({ value: input.value + 1 });
            });
            "#,
            json!({ "value": 4 }),
        )
        .unwrap();

        assert_eq!(output, json!({ "value": 5 }));
    }

    #[test]
    fn provides_crypto_random_uuid_without_host_access() {
        let output = run(
            r#"
            rubick.tool.onInvoke(function () {
              return { id: crypto.randomUUID() };
            });
            "#,
            json!({}),
        )
        .unwrap();

        let id = output["id"].as_str().unwrap();
        assert!(uuid::Uuid::parse_str(id).is_ok());
    }

    #[test]
    fn rejects_missing_tool_callback() {
        let error = run("var answer = 42;", json!({})).unwrap_err();
        assert!(error.to_string().contains("did not register"));
    }

    #[test]
    fn rejects_non_json_result() {
        let error = run(
            r#"
            rubick.tool.onInvoke(function () {
              var value = {};
              value.self = value;
              return value;
            });
            "#,
            json!({}),
        )
        .unwrap_err();

        assert!(error.to_string().contains("not JSON serializable"));
    }

    #[test]
    fn interrupts_runaway_javascript() {
        let started = Instant::now();
        let error = execute_source_with_limits(
            r#"
            rubick.tool.onInvoke(function () {
              while (true) {}
            });
            "#,
            json!({}),
            Duration::from_millis(20),
            8 * 1024 * 1024,
        )
        .unwrap_err();

        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(error.to_string().contains("timed out"));
    }

    #[tokio::test]
    async fn renderer_tool_is_rejected_by_headless_entrypoint() {
        let (plugin, tool) = plugin_and_tool(ToolRuntime::Renderer);
        let error = execute_headless_tool(plugin, tool, json!({}))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("renderer runtime"));
    }
}
