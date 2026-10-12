//! The execution service as the knowledge service sees it (CONTRACT.md
//! §22): after a scan, the environments a repository declares in `.sdi.yml`
//! are handed over with `SyncEnvironments`, and the workspace's registries
//! are read to resolve the one an environment names.
//!
//! Behind a trait so the scan is testable without the peer running. Every
//! call carries a principal as metadata and is bounded by a short timeout:
//! an execution outage must never hold a scan.

use std::time::Duration;

use sdi_common::principal::Principal;
use sdi_proto::execution::v1::{
    Environment, ListRegistriesRequest, Registry, SyncEnvironmentsRequest,
    execution_service_client::ExecutionServiceClient,
};
use tonic::{Request, Status, transport::Channel};

/// Both calls are one or two statements on the other side.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);

#[tonic::async_trait]
pub trait DeliveryGateway: Send + Sync {
    /// The registries of the principal's workspace.
    async fn list_registries(&self, principal: &Principal) -> Result<Vec<Registry>, Status>;

    /// Replaces the config-managed environments of `project_id` with
    /// `environments` (possibly none) and returns the project's environments.
    async fn sync_environments(
        &self,
        principal: &Principal,
        project_id: &str,
        environments: Vec<Environment>,
    ) -> Result<Vec<Environment>, Status>;
}

/// The real client over a lazily connected channel: the knowledge service
/// starts without execution and finds it once it is up.
#[derive(Clone)]
pub struct GrpcExecution {
    client: ExecutionServiceClient<Channel>,
}

impl GrpcExecution {
    pub fn connect(url: &str) -> anyhow::Result<Self> {
        let channel = sdi_common::grpc::lazy_channel(url)?;
        Ok(Self {
            client: ExecutionServiceClient::new(channel),
        })
    }

    fn request<T>(principal: &Principal, message: T) -> Request<T> {
        let mut request = Request::new(message);
        principal.apply(&mut request);
        request.set_timeout(CALL_TIMEOUT);
        request
    }
}

async fn timed<T>(call: impl Future<Output = Result<T, Status>>) -> Result<T, Status> {
    match tokio::time::timeout(CALL_TIMEOUT, call).await {
        Ok(result) => result,
        Err(_) => Err(Status::deadline_exceeded("execution service timed out")),
    }
}

#[tonic::async_trait]
impl DeliveryGateway for GrpcExecution {
    async fn list_registries(&self, principal: &Principal) -> Result<Vec<Registry>, Status> {
        let mut client = self.client.clone();
        let request = Self::request(principal, ListRegistriesRequest {});
        Ok(timed(client.list_registries(request))
            .await?
            .into_inner()
            .registries)
    }

    async fn sync_environments(
        &self,
        principal: &Principal,
        project_id: &str,
        environments: Vec<Environment>,
    ) -> Result<Vec<Environment>, Status> {
        let mut client = self.client.clone();
        let request = Self::request(
            principal,
            SyncEnvironmentsRequest {
                project_id: project_id.to_owned(),
                environments,
            },
        );
        Ok(timed(client.sync_environments(request))
            .await?
            .into_inner()
            .environments)
    }
}

#[cfg(test)]
pub mod fake {
    //! An execution service that remembers what it was told.

    use std::sync::Mutex;

    use super::*;

    /// One `SyncEnvironments` call: who asked, for which project, with what.
    #[derive(Debug, Clone)]
    pub struct SyncCall {
        pub principal: Principal,
        pub project_id: String,
        pub environments: Vec<Environment>,
    }

    #[derive(Default)]
    pub struct FakeExecution {
        pub registries: Vec<Registry>,
        /// When set, every call answers this status code.
        pub fail_with: Option<tonic::Code>,
        pub synced: Mutex<Vec<SyncCall>>,
        pub registry_lookups: Mutex<Vec<Principal>>,
    }

    impl FakeExecution {
        fn check(&self) -> Result<(), Status> {
            match self.fail_with {
                Some(code) => Err(Status::new(code, "execution is down")),
                None => Ok(()),
            }
        }

        pub fn calls(&self) -> Vec<SyncCall> {
            self.synced.lock().unwrap().clone()
        }
    }

    #[tonic::async_trait]
    impl DeliveryGateway for FakeExecution {
        async fn list_registries(&self, principal: &Principal) -> Result<Vec<Registry>, Status> {
            self.registry_lookups
                .lock()
                .unwrap()
                .push(principal.clone());
            self.check()?;
            Ok(self.registries.clone())
        }

        async fn sync_environments(
            &self,
            principal: &Principal,
            project_id: &str,
            environments: Vec<Environment>,
        ) -> Result<Vec<Environment>, Status> {
            self.synced.lock().unwrap().push(SyncCall {
                principal: principal.clone(),
                project_id: project_id.to_owned(),
                environments: environments.clone(),
            });
            self.check()?;
            Ok(environments)
        }
    }
}
