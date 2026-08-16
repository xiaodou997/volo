//! AI 模块
//! LLM 接入（OpenAI 兼容协议）+ 内置工具注册表 + Agent 会话原型
//!
//! 依赖方向：Agent → ChatBackend / ToolRegistry → PermissionEngine → Capability

pub mod agent;
pub mod llm;
pub mod tools;
