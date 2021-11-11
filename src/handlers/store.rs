use crate::{
    proto,
    store::{get_obj, set_obj, Store},
    Error,
};
use std::{
    sync::{Arc, Mutex},
    time::{SystemTime, SystemTimeError, UNIX_EPOCH},
};

pub struct StoreServiceImpl {
    id: String,
    store: Arc<Mutex<Box<dyn Store>>>,
}

impl StoreServiceImpl {
    pub fn new<S: Store>(id: String, store: S) -> Self {
        StoreServiceImpl {
            id,
            store: Arc::new(Mutex::new(Box::new(store))),
        }
    }

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

impl proto::StoreService for StoreServiceImpl {
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

    fn get(
        &self,
        _: grpc::RequestOptions,
        req: proto::GetRequest,
    ) -> grpc::SingleResponse<proto::GetResponse> {
        let key = req.key.as_str();

        let store_map = self.store.clone();
        let store = store_map.lock().unwrap();

        let value_opt = match get_obj(store.as_ref(), key) {
            Ok(v) => v,
            Err(e) => return error_response(e.into()),
        };

        let value = match value_opt {
            Some(v) => v,
            None => return error_response(Error::NotFound.into()),
        };

        let response = proto::GetResponse {
            key: key.to_owned(),
            value,
            ..Default::default()
        };
        grpc::SingleResponse::completed(response)
    }

    fn set(
        &self,
        _: grpc::RequestOptions,
        req: proto::SetRequest,
    ) -> grpc::SingleResponse<proto::SetResponse> {
        let store_map = self.store.clone();
        let mut store = store_map.lock().unwrap();

        if let Err(e) = set_obj(store.as_mut(), req.key.as_str(), req.value.clone()) {
            return error_response(e.into());
        }

        let response = proto::SetResponse {
            key: req.key,
            value: req.value,
            ..Default::default()
        };
        grpc::SingleResponse::completed(response)
    }
}
