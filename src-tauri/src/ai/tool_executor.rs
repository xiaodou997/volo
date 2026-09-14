//! 统一工具执行抽象。
//!
//! Agent 与 Workflow 都依赖这个接口，而不是彼此依赖。具体生产实现仍由
//! `plugin_tools::AgentToolExecutor` 提供，负责 builtin / plugin / MCP 路由。

use std::future::Future;
use std::pin::Pin;

use serde_json::Value;

use crate::error::Result;

pub trait ToolExecutor: Send + Sync {
    fn execute<'a>(
        &'a self,
        name: &'a str,
        args: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>>;
}
