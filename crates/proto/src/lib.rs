//! Generated gRPC contracts. The `.proto` files under `/proto` are the source
//! of truth for every service-to-service call; nothing here is hand-written.

pub mod common {
    pub mod v1 {
        tonic::include_proto!("sdi.common.v1");
    }
}

pub mod knowledge {
    pub mod v1 {
        tonic::include_proto!("sdi.knowledge.v1");
    }
}

pub mod intelligence {
    pub mod v1 {
        tonic::include_proto!("sdi.intelligence.v1");
    }
}

pub mod execution {
    pub mod v1 {
        tonic::include_proto!("sdi.execution.v1");
    }
}

pub mod identity {
    pub mod v1 {
        tonic::include_proto!("sdi.identity.v1");
    }
}

pub mod gitops {
    pub mod v1 {
        tonic::include_proto!("sdi.gitops.v1");
    }
}
