pub mod kvtest;
pub mod store;

mod raft;

use std::collections::HashMap;

use crate::error::Error;
use crate::handlers::store::StoreServiceImpl;
use crate::proto;
use crate::raft::Raft;
use crate::sql::Storage;
use crate::state::State;
use crate::store::{File, KVMemory};

pub struct Node {
    pub id: String,
    pub addr: String,
    pub threads: usize,
    pub peers: HashMap<String, std::net::SocketAddr>,
    pub data_dir: String,
}

impl Node {
    pub fn listen(&self) -> Result<(), Error> {
        info!("Starting node with ID {}", self.id);
        let mut server = grpc::ServerBuilder::new_plain();
        server.http.set_addr(&self.addr)?;
        server.http.set_cpu_pool_threads(self.threads);

        let data_path = std::path::Path::new(&self.data_dir);
        std::fs::create_dir_all(data_path)?;

        let raft_transport = raft::GRPC::new(self.peers.clone())?;
        server.add_service(proto::RaftServer::new_service_def(
            raft_transport.build_service()?,
        ));

        let state_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(data_path.join("statef"))?;

        let raft_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(data_path.join("raft"))?;

        let raft = Raft::start(
            &self.id,
            self.peers.keys().cloned().collect(),
            State::new(File::new(state_file)?),
            File::new(raft_file)?,
            raft_transport,
        )?;

        server.add_service(proto::StoreServiceServer::new_service_def(
            StoreServiceImpl {
                id: self.id.clone(),
                raft: raft.clone(),
                storage: Box::new(Storage::new(KVMemory::new())),
            },
        ));
        let _s = server.build()?;

        raft.join()
    }
}
