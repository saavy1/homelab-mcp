#![allow(dead_code)]

use std::time::Instant;
use tracing::{Span, info, info_span, warn};

pub struct ToolCall {
    tool: &'static str,
    request_id: String,
    started_at: Instant,
    span: Span,
}

impl ToolCall {
    pub fn start(tool: &'static str, request_id: impl Into<String>) -> Self {
        let request_id = request_id.into();
        let span = info_span!("mcp_tool_call", tool, request_id = %request_id);
        let cloned_span = span.clone();
        let _entered = cloned_span.enter();
        info!(event = "tool_call_started", tool, request_id = %request_id);
        Self {
            tool,
            request_id,
            started_at: Instant::now(),
            span,
        }
    }

    pub fn span(&self) -> Span {
        self.span.clone()
    }

    pub fn complete(
        &self,
        service: &'static str,
        operation: &'static str,
        affected_id: Option<&str>,
    ) {
        info!(
            event = "tool_call_completed",
            tool = self.tool,
            request_id = %self.request_id,
            service,
            operation,
            affected_id,
            latency_ms = self.started_at.elapsed().as_millis() as u64
        );
    }

    pub fn fail(
        &self,
        service: &'static str,
        operation: &'static str,
        status: Option<u16>,
        retryable: bool,
    ) {
        warn!(
            event = "tool_call_failed",
            tool = self.tool,
            request_id = %self.request_id,
            service,
            operation,
            status,
            retryable,
            latency_ms = self.started_at.elapsed().as_millis() as u64
        );
    }
}
