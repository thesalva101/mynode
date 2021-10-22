use std::collections::HashMap;

use crate::error::Error;
use crate::handlers::store::StoreServiceImpl;
use crate::proto;
use crate::store::File;

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

        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(data_path.join("state"))?;

        server.add_service(proto::StoreServiceServer::new_service_def(
            StoreServiceImpl::new(self.id.clone(), File::new(file)?),
        ));
        let _s = server.build()?;

        loop {
            std::thread::park();
        }
    }
}
