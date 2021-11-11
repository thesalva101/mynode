use crate::raft::Raft;
use crate::serializer::{deserialize, serialize};
use crate::state::{Mutation, Read};
use crate::{
    proto,
    store::{get_obj, set_obj},
    Error,
};
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

pub struct StoreRaftServiceImpl {
    pub id: String,
    pub raft: Raft,
}

impl StoreRaftServiceImpl {
    fn get_timestamp(&self) -> Result<i64, SystemTimeError> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|t| t.as_secs() as i64)
    }
}

fn error_response<T: Send>(error: Box<dyn std::error::Error>) -> grpc::SingleResponse<T> {
    let grpc_error = grpc::Error::Panic(format!("{}", error));
    grpc::SingleResponse::err(grpc_error)
}

impl proto::StoreService for StoreRaftServiceImpl {
    fn echo(
        &self,
        _: grpc::RequestOptions,
        req: proto::EchoRequest,
    ) -> grpc::SingleResponse<proto::EchoResponse> {
        let value = req.value.clone();
        grpc::SingleResponse::completed(proto::EchoResponse {
            value,
            ..Default::default()
        })
    }

    fn status(
        &self,
        _: grpc::RequestOptions,
        _: proto::StatusRequest,
    ) -> grpc::SingleResponse<proto::StatusResponse> {
        let time = match self.get_timestamp() {
            Ok(t) => t,
            Err(e) => return error_response(e.into()),
        };

        let response = proto::StatusResponse {
            id: self.id.clone(),
            version: env!("CARGO_PKG_VERSION").into(),
            time: time,
            ..Default::default()
        };
        grpc::SingleResponse::completed(response)
    }

    fn set(
        &self,
        _: grpc::RequestOptions,
        req: proto::SetRequest,
    ) -> grpc::SingleResponse<proto::SetResponse> {
        self.raft
            .mutate(
                serialize(Mutation::Set {
                    key: req.key,
                    value: serialize(req.value).unwrap(),
                })
                .unwrap(),
            )
            .unwrap();
        grpc::SingleResponse::completed(proto::SetResponse {
            ..Default::default()
        })
    }

    fn get(
        &self,
        _: grpc::RequestOptions,
        req: proto::GetRequest,
    ) -> grpc::SingleResponse<proto::GetResponse> {
        let key_bytes = serialize(Read::Get(req.key.clone())).unwrap();
        let value = match self.raft.read(key_bytes) {
            Ok(value_bytes) => deserialize(value_bytes).unwrap(),
            Err(e) => return error_response(e.into()),
        };
        let response = proto::GetResponse {
            key: req.key,
            value,
            ..Default::default()
        };
        grpc::SingleResponse::completed(response)
    }
}
