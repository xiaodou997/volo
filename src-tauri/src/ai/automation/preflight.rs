use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::ai::mcp::MCP_NAME_PREFIX;
use crate::ai::plugin_tools::{lookup_tool, PLUGIN_NAME_PREFIX};
use crate::ai::tools::ToolRegistry;
use crate::ai::workflow::{Workflow, WorkflowStep};
use crate::core::capability::{capability_meta, RiskLevel};
use crate::core::permission::{GrantInfo, PermissionEngine, Scope};
use crate::error::{Result, VoloError};
use crate::plugin::manager::PluginState;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreflightStatus {
    Ready,
    Missing,
    Runtime,
    Invalid,
    Unsupported,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionPreflightRequirement {
    pub step_id: String,
    pub tool_name: String,
    pub capability: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    pub risk: &'static str,
    pub description: String,
    pub status: PreflightStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationPermissionPreflight {
    pub workflow_id: String,
    pub ready: bool,
    pub fully_verified: bool,
    pub ready_count: usize,
    pub missing_count: usize,
    pub runtime_count: usize,
    pub blocker_count: usize,
    pub requirements: Vec<PermissionPreflightRequirement>,
}

fn risk_name(risk: RiskLevel) -> &'static str {
    match risk {
        RiskLevel::Low => "low",
        RiskLevel::Medium => "medium",
        RiskLevel::High => "high",
        RiskLevel::Critical => "critical",
    }
}

fn normalize_resource(resource: Option<&str>) -> Option<String> {
    resource.map(|value| value.replace('\\', "/"))
}

fn has_always_grant(
    grants: &[GrantInfo],
    principal: &str,
    capability: &str,
    resource: Option<&str>,
) -> bool {
    let resource = normalize_resource(resource);
    grants.iter().any(|grant| {
        grant.plugin_id == principal
            && grant.capability == capability
            && grant.scope == Scope::Always
            && normalize_resource(grant.resource.as_deref()) == resource
    })
}

fn contains_runtime_binding(value: &str) -> bool {
    value.contains("${")
}

fn builtin_resource_arg<'a>(name: &str, args: &'a Value) -> Option<&'a str> {
    let field = match name {
        "fs_read" | "fs_write" => "path",
        "shell_open" => "target",
        "skill_load" => "name",
        _ => return None,
    };
    args.get(field).and_then(Value::as_str)
}

fn requirement(
    step_id: &str,
    tool_name: &str,
    capability: &str,
    resource: Option<String>,
    status: PreflightStatus,
    note: Option<String>,
) -> PermissionPreflightRequirement {
    let meta = capability_meta(capability);
    PermissionPreflightRequirement {
        step_id: step_id.to_string(),
        tool_name: tool_name.to_string(),
        capability: capability.to_string(),
        resource,
        risk: risk_name(meta.risk),
        description: meta.description.to_string(),
        status,
        note,
    }
}

fn analyze_builtin(
    principal: &str,
    grants: &[GrantInfo],
    step_id: &str,
    name: &str,
    args: &Value,
) -> PermissionPreflightRequirement {
    let Some(capability) = ToolRegistry::capability_of(name) else {
        return requirement(
            step_id,
            name,
            "unknown",
            None,
            PreflightStatus::Unsupported,
            Some(format!("未注册的内置 Tool: {}", name)),
        );
    };

    let meta = capability_meta(capability);
    if meta.risk == RiskLevel::Low {
        return requirement(
            step_id,
            name,
            capability,
            None,
            PreflightStatus::Ready,
            Some("Low 风险能力无需后台 Always grant".to_string()),
        );
    }

    if builtin_resource_arg(name, args)
        .is_some_and(contains_runtime_binding)
    {
        return requirement(
            step_id,
            name,
            capability,
            None,
            PreflightStatus::Runtime,
            Some("资源包含 Workflow 动态绑定，需运行时解析后再次校验".to_string()),
        );
    }

    let resource = match ToolRegistry::resource_for(name, args) {
        Ok(resource) => resource,
        Err(error) => {
            return requirement(
                step_id,
                name,
                capability,
                None,
                PreflightStatus::Invalid,
                Some(error.to_string()),
            )
        }
    };

    let status = if has_always_grant(grants, principal, capability, resource.as_deref()) {
        PreflightStatus::Ready
    } else {
        PreflightStatus::Missing
    };

    requirement(step_id, name, capability, resource, status, None)
}

fn analyze_mcp(
    principal: &str,
    grants: &[GrantInfo],
    step_id: &str,
    name: &str,
) -> PermissionPreflightRequirement {
    let capability = format!("mcp.call:{}", name);
    let status = if has_always_grant(grants, principal, &capability, Some(name)) {
        PreflightStatus::Ready
    } else {
        PreflightStatus::Missing
    };
    requirement(
        step_id,
        name,
        &capability,
        Some(name.to_string()),
        status,
        None,
    )
}

fn analyze_plugin(
    plugins: &PluginState,
    step_id: &str,
    name: &str,
) -> Vec<PermissionPreflightRequirement> {
    let Some((plugin_id, _tool_id)) = lookup_tool(plugins, name) else {
        return vec![requirement(
            step_id,
            name,
            "plugin.runtime",
            None,
            PreflightStatus::Unsupported,
            Some("Plugin Tool 不存在或插件未启用".to_string()),
        )];
    };
    let Some(plugin) = plugins.get_plugin(&plugin_id) else {
        return vec![requirement(
            step_id,
            name,
            "plugin.runtime",
            None,
            PreflightStatus::Unsupported,
            Some("Plugin Tool 对应插件不可用".to_string()),
        )];
    };

    let mut requirements = Vec::new();
    for permission in &plugin.permissions {
        let meta = capability_meta(permission);
        if meta.risk == RiskLevel::Low {
            continue;
        }
        requirements.push(requirement(
            step_id,
            name,
            permission,
            None,
            PreflightStatus::Runtime,
            Some(
                "Plugin Tool 的实际宿主调用与资源由脚本运行时决定；Scheduler 仍会逐次执行 manifest + Always 校验"
                    .to_string(),
            ),
        ));
    }

    requirements
}

pub fn analyze(app: &AppHandle, workflow: &Workflow) -> Result<AutomationPermissionPreflight> {
    let principal = crate::ai::workflow::commands::workflow_principal(&workflow.id);
    let engine = app.state::<PermissionEngine>();
    let grants = engine.list_grants()?;
    let plugins = app.state::<PluginState>();

    let mut requirements = Vec::new();

    for step in &workflow.steps {
        let WorkflowStep::Tool {
            id,
            name,
            args,
        } = step
        else {
            continue;
        };

        if name.starts_with(MCP_NAME_PREFIX) {
            requirements.push(analyze_mcp(&principal, &grants, id, name));
        } else if name.starts_with(PLUGIN_NAME_PREFIX) {
            requirements.extend(analyze_plugin(&plugins, id, name));
        } else {
            requirements.push(analyze_builtin(&principal, &grants, id, name, args));
        }
    }

    // Low-risk Ready 项保留在明细中，让用户能看到为什么“不需要授权”。
    let ready_count = requirements
        .iter()
        .filter(|item| item.status == PreflightStatus::Ready)
        .count();
    let missing_count = requirements
        .iter()
        .filter(|item| item.status == PreflightStatus::Missing)
        .count();
    let runtime_count = requirements
        .iter()
        .filter(|item| item.status == PreflightStatus::Runtime)
        .count();
    let blocker_count = requirements
        .iter()
        .filter(|item| {
            matches!(
                item.status,
                PreflightStatus::Invalid | PreflightStatus::Unsupported
            )
        })
        .count();

    Ok(AutomationPermissionPreflight {
        workflow_id: workflow.id.clone(),
        ready: missing_count == 0 && blocker_count == 0,
        fully_verified: missing_count == 0 && blocker_count == 0 && runtime_count == 0,
        ready_count,
        missing_count,
        runtime_count,
        blocker_count,
        requirements,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(
        principal: &str,
        capability: &str,
        resource: Option<&str>,
        scope: Scope,
    ) -> GrantInfo {
        let meta = capability_meta(capability);
        GrantInfo {
            plugin_id: principal.to_string(),
            capability: capability.to_string(),
            resource: resource.map(str::to_string),
            scope,
            risk: meta.risk,
            description: meta.description,
        }
    }

    #[test]
    fn always_grant_matching_is_exact_and_resource_scoped() {
        let grants = vec![
            grant(
                "workflow:a",
                "fs.read",
                Some("/tmp/input.txt"),
                Scope::Always,
            ),
            grant("workflow:a", "clipboard.read", None, Scope::Session),
        ];

        assert!(has_always_grant(
            &grants,
            "workflow:a",
            "fs.read",
            Some("/tmp/input.txt")
        ));
        assert!(!has_always_grant(
            &grants,
            "workflow:a",
            "fs.read",
            Some("/tmp/other.txt")
        ));
        assert!(!has_always_grant(
            &grants,
            "workflow:a",
            "clipboard.read",
            None
        ));
    }

    #[test]
    fn workflow_binding_is_runtime_dependent() {
        assert!(contains_runtime_binding("${input.path}"));
        assert!(contains_runtime_binding("/tmp/${steps.pick}"));
        assert!(!contains_runtime_binding("/tmp/input.txt"));
    }
}
