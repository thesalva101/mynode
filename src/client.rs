use crate::proto;
use crate::Error;
use grpc::ClientStubExt;
use proto::StoreService;

/// A Store client
pub struct Client {
    client: proto::StoreServiceClient,
}

impl Client {
    /// Creates a new client
    pub fn new(addr: std::net::SocketAddr) -> Result<Self, Error> {
        Ok(Self {
            client: proto::StoreServiceClient::new_plain(
                &addr.ip().to_string(),
                addr.port(),
                grpc::ClientConf::new(),
            )?,
        })
    }

    /// Runs an echo request
    pub fn echo(&self, value: &str) -> Result<String, Error> {
        let (_, resp, _) = self
            .client
            .echo(
                grpc::RequestOptions::new(),
                proto::EchoRequest {
                    value: value.to_owned(),
                    ..Default::default()
                },
            )
            .wait()?;
        Ok(resp.value)
    }
}
