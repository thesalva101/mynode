use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

use grpc::{RequestOptions, StreamingResponse};

use crate::proto::QueryRequest;
use crate::raft::Raft;
use crate::serializer::{deserialize, serialize};
use crate::sql::types::{Row, Value};
use crate::state::{Mutation, Read};
use crate::{
    proto,
    store::{get_obj, set_obj},
    Error,
};

pub struct StoreRaftServiceImpl {
    pub id: String,
    pub raft: Raft,
}

fn error_response<T: Send>(error: Box<dyn std::error::Error>) -> grpc::SingleResponse<T> {
    let grpc_error = grpc::Error::Panic(format!("{}", error));
    grpc::SingleResponse::err(grpc_error)
}

impl proto::StoreService for StoreRaftServiceImpl {
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

    fn query(&self, _: RequestOptions, _req: QueryRequest) -> StreamingResponse<proto::Row> {
        let mut metadata = grpc::Metadata::new();
        metadata.add(
            grpc::MetadataKey::from("columns"),
            serialize(vec!["null", "boolean", "integer", "float", "string"])
                .unwrap()
                .into(),
        );
        let rows = vec![Self::row_to_protobuf(vec![
            Value::Null,
            Value::Boolean(true),
            Value::Integer(7),
            Value::Float(3.145),
            Value::String("Hi! 👏".into()),
        ])];
        grpc::StreamingResponse::iter_with_metadata(metadata, rows.into_iter())
    }
}

impl StoreRaftServiceImpl {
    fn get_timestamp(&self) -> Result<i64, SystemTimeError> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|t| t.as_secs() as i64)
    }

    /// Converts a row into a protobuf row
    fn row_to_protobuf(row: Row) -> proto::Row {
        proto::Row {
            field: row.into_iter().map(Self::value_to_protobuf).collect(),
            ..Default::default()
        }
    }

    /// Converts a value into a protobuf field
    fn value_to_protobuf(value: Value) -> proto::Field {
        proto::Field {
            value: match value {
                Value::Null => None,
                Value::Boolean(b) => Some(proto::Field_oneof_value::boolean(b)),
                Value::Float(f) => Some(proto::Field_oneof_value::float(f)),
                Value::Integer(i) => Some(proto::Field_oneof_value::integer(i)),
                Value::String(s) => Some(proto::Field_oneof_value::string(s)),
            },
            ..Default::default()
        }
    }
}
