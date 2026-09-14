use serde_json::Value;

use crate::error::{Result, VoloError};

use super::WorkflowContext;

const INPUT_REF: &str = "${input}";
const PREVIOUS_REF: &str = "${previous}";
const STEP_REF_PREFIX: &str = "${steps.";

/// 递归解析 Workflow Tool args 中的精确数据引用。
///
/// 仅当字符串值完整等于 `${input}` / `${previous}` / `${steps.<id>}` 时替换，
/// 因此替换结果可保持 JSON 原始类型；普通字符串与非字符串值原样保留。
pub(crate) fn resolve_workflow_value(value: &Value, context: &WorkflowContext) -> Result<Value> {
    match value {
        Value::String(text) => resolve_string(text, context),
        Value::Array(items) => items
            .iter()
            .map(|item| resolve_workflow_value(item, context))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| {
                resolve_workflow_value(value, context).map(|resolved| (key.clone(), resolved))
            })
            .collect::<Result<serde_json::Map<String, Value>>>()
            .map(Value::Object),
        _ => Ok(value.clone()),
    }
}

fn resolve_string(text: &str, context: &WorkflowContext) -> Result<Value> {
    if text == INPUT_REF {
        return Ok(context.input().clone());
    }
    if text == PREVIOUS_REF {
        return context.previous_output().cloned().ok_or_else(|| {
            VoloError::Other("workflow 引用 ${previous} 时还没有上一步输出".to_string())
        });
    }
    if let Some(step_id) = text
        .strip_prefix(STEP_REF_PREFIX)
        .and_then(|rest| rest.strip_suffix('}'))
    {
        if step_id.is_empty() {
            return Err(VoloError::Other(
                "workflow step 引用缺少 step id: ${steps.}".to_string(),
            ));
        }
        return context.output(step_id).cloned().ok_or_else(|| {
            VoloError::Other(format!("workflow 找不到 step 输出: {}", step_id))
        });
    }
    Ok(Value::String(text.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn context() -> WorkflowContext {
        let mut context = WorkflowContext::new(json!({
            "path": "/tmp/input.txt",
            "enabled": true,
        }));
        context.record("read", json!({ "text": "hello", "count": 2 }));
        context.record("summary", json!(["a", "b"]));
        context
    }

    #[test]
    fn exact_references_preserve_json_types() {
        let context = context();
        assert_eq!(
            resolve_workflow_value(&json!("${input}"), &context).unwrap(),
            json!({ "path": "/tmp/input.txt", "enabled": true })
        );
        assert_eq!(
            resolve_workflow_value(&json!("${previous}"), &context).unwrap(),
            json!(["a", "b"])
        );
        assert_eq!(
            resolve_workflow_value(&json!("${steps.read}"), &context).unwrap(),
            json!({ "text": "hello", "count": 2 })
        );
    }

    #[test]
    fn nested_values_are_resolved_recursively_without_touching_keys() {
        let context = context();
        let value = json!({
            "payload": "${steps.read}",
            "items": ["literal", "${previous}", 42],
            "${input}": "key stays literal"
        });
        assert_eq!(
            resolve_workflow_value(&value, &context).unwrap(),
            json!({
                "payload": { "text": "hello", "count": 2 },
                "items": ["literal", ["a", "b"], 42],
                "${input}": "key stays literal"
            })
        );
    }

    #[test]
    fn embedded_tokens_remain_literal_strings() {
        let context = context();
        assert_eq!(
            resolve_workflow_value(&json!("prefix ${input}"), &context).unwrap(),
            json!("prefix ${input}")
        );
    }

    #[test]
    fn missing_previous_or_step_output_fails_closed() {
        let empty = WorkflowContext::new(Value::Null);
        assert!(resolve_workflow_value(&json!("${previous}"), &empty)
            .unwrap_err()
            .to_string()
            .contains("还没有上一步输出"));
        assert!(resolve_workflow_value(&json!("${steps.missing}"), &empty)
            .unwrap_err()
            .to_string()
            .contains("找不到 step 输出"));
    }
}
