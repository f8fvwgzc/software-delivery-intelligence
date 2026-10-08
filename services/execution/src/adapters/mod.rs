//! Tool adapters. `simulated` is the only one implemented in v1; the
//! reserved names resolve to a stub that answers UNIMPLEMENTED so a plan
//! naming them fails loudly instead of silently dry-running.

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use tonic::Status;

pub mod simulated;

pub use simulated::SimulatedAdapter;

pub const SIMULATED: &str = "simulated";
pub const RESERVED: [&str; 4] = ["github", "docker", "cloud", "kubernetes"];

/// What the adapter needs to know about the step it runs.
#[derive(Debug, Clone)]
pub struct StepInput {
    pub key: String,
    pub title: String,
    pub action_type: String,
    /// Adapter the plan named for this step ("docker", "cloud", ...). The
    /// simulated adapter uses it to pick a believable output shape.
    pub adapter: String,
    pub params: BTreeMap<String, String>,
    pub attempt: u32,
}

/// Per-run context: the dry-run fault to honour, if any.
#[derive(Debug, Clone, Default)]
pub struct RunContext {
    pub fault: Option<Fault>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Fault {
    pub step_key: String,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    Succeeded { output: String },
    Failed { code: String, raw_output: String },
}

#[async_trait]
pub trait Adapter: Send + Sync {
    fn name(&self) -> &'static str;
    async fn run(&self, step: &StepInput, ctx: &RunContext) -> StepOutcome;
}

/// Resolves an adapter name. Unknown names are INVALID_ARGUMENT, reserved
/// ones UNIMPLEMENTED.
#[derive(Clone)]
pub struct Registry {
    simulated: Arc<dyn Adapter>,
}

impl Registry {
    pub fn new(simulated: Arc<dyn Adapter>) -> Self {
        Self { simulated }
    }

    pub fn get(&self, name: &str) -> Result<Arc<dyn Adapter>, Status> {
        match name {
            SIMULATED => Ok(self.simulated.clone()),
            reserved if RESERVED.contains(&reserved) => Err(Status::unimplemented(format!(
                "adapter {reserved} is reserved and not implemented yet; use {SIMULATED}"
            ))),
            other => Err(Status::invalid_argument(format!(
                "unknown adapter: {other}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_resolves_simulated_and_refuses_the_rest() {
        let registry = Registry::new(Arc::new(SimulatedAdapter::new(1)));
        assert_eq!(registry.get(SIMULATED).unwrap().name(), SIMULATED);
        for name in RESERVED {
            let code = registry.get(name).err().expect("reserved").code();
            assert_eq!(code, tonic::Code::Unimplemented);
        }
        let code = registry.get("mystery").err().expect("unknown").code();
        assert_eq!(code, tonic::Code::InvalidArgument);
    }
}
