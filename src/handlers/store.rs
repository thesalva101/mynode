use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

use grpc::{RequestOptions, StreamingResponse};

use crate::proto;
use crate::proto::QueryRequest;
use crate::raft::Raft;
use crate::serializer::serialize;
use crate::sql::types::{Row, Value};
use crate::sql::{Parser, Planner, Storage};

pub struct StoreServiceImpl {
    pub id: String,
    pub raft: Raft,
    pub storage: Box<Storage>,
}

fn error_response<T: Send>(error: Box<dyn std::error::Error>) -> grpc::SingleResponse<T> {
    let grpc_error = grpc::Error::Panic(format!("{}", error));
    grpc::SingleResponse::err(grpc_error)
}

impl proto::StoreService for StoreServiceImpl {
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

    fn query(&self, _: RequestOptions, req: QueryRequest) -> StreamingResponse<proto::Row> {
        let plan = Planner::new(self.storage.clone())
            .build(Parser::new(&req.query).parse().unwrap())
            .unwrap();
        let mut metadata = grpc::Metadata::new();
        metadata.add(
            grpc::MetadataKey::from("columns"),
            serialize(&plan.columns).unwrap().into(),
        );
        // TODO: FIXME This needs to handle errors
        grpc::StreamingResponse::iter_with_metadata(
            metadata,
            plan.map(|row| Self::row_to_protobuf(row.unwrap())),
        )
    }

    fn get_table(
        &self,
        _: grpc::RequestOptions,
        req: proto::GetTableRequest,
    ) -> grpc::SingleResponse<proto::GetTableResponse> {
        let schema = self.storage.get_table(&req.name).unwrap();
        grpc::SingleResponse::completed(proto::GetTableResponse {
            sql: schema.to_query(),
            ..Default::default()
        })
    }
}

impl StoreServiceImpl {
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
