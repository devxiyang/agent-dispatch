mod backend;
mod error;
mod providers;

use dispatch_core::BackendKind;

pub use backend::{
    AgentBackend, Availability, BackendCapabilities, ForkPlan, ResumePlan, ResumeSpec, StartPlan,
    StartSpec,
};
pub use error::{BackendError, Result};
pub use providers::{ClaudeBackend, CodexBackend, CursorAgentBackend, PiBackend};

pub fn backend_for(kind: &BackendKind) -> Box<dyn AgentBackend> {
    match kind {
        BackendKind::Codex => Box::<CodexBackend>::default(),
        BackendKind::ClaudeCode => Box::<ClaudeBackend>::default(),
        BackendKind::Pi => Box::<PiBackend>::default(),
        BackendKind::CursorAgent => Box::<CursorAgentBackend>::default(),
        BackendKind::Generic => Box::<CursorAgentBackend>::default(),
    }
}

pub fn all_backends() -> Vec<Box<dyn AgentBackend>> {
    vec![
        Box::<CodexBackend>::default(),
        Box::<ClaudeBackend>::default(),
        Box::<PiBackend>::default(),
        Box::<CursorAgentBackend>::default(),
    ]
}
