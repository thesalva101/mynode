use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

use grpc::{RequestOptions, StreamingResponse};

use crate::proto::QueryRequest;
use crate::raft::Raft;
use crate::serializer::serialize;
use crate::sql::types::{Row, Value};
use crate::sql::{Parser, Plan, Planner, Storage};
use crate::{proto, Error};

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
        let response = proto::StatusResponse {
            id: self.id.clone(),
            version: env!("CARGO_PKG_VERSION").into(),
            ..Default::default()
        };
        grpc::SingleResponse::completed(response)
    }

    fn query(&self, _: RequestOptions, req: QueryRequest) -> StreamingResponse<proto::Row> {
        let plan = match self.execute(&req.query) {
            Ok(plan) => plan,
            Err(err) => {
                return grpc::StreamingResponse::completed(vec![proto::Row {
                    error: Self::error_to_protobuf(err),
                    ..Default::default()
                }])
            }
        };
        let mut metadata = grpc::Metadata::new();
        metadata.add(
            grpc::MetadataKey::from("columns"),
            serialize(&plan.columns).unwrap().into(),
        );
        grpc::StreamingResponse::iter_with_metadata(
            metadata,
            plan.map(|result| match result {
                Ok(row) => Self::row_to_protobuf(row),
                Err(err) => proto::Row {
                    error: Self::error_to_protobuf(err),
                    ..Default::default()
                },
            }),
        )
    }

    fn get_table(
        &self,
        _: grpc::RequestOptions,
        req: proto::GetTableRequest,
    ) -> grpc::SingleResponse<proto::GetTableResponse> {
        let mut resp = proto::GetTableResponse::new();
        match self.storage.get_table(&req.name) {
            Ok(schema) => resp.sql = schema.to_query(),
            Err(err) => resp.error = Self::error_to_protobuf(err),
        };
        grpc::SingleResponse::completed(resp)
    }
}

impl StoreServiceImpl {
    /// Executes an SQL statement
    fn execute(&self, query: &str) -> Result<Plan, Error> {
        Planner::new(self.storage.clone()).build(Parser::new(query).parse()?)
    }

    /// Converts an error into a protobuf object
    fn error_to_protobuf(err: Error) -> protobuf::SingularPtrField<proto::Error> {
        protobuf::SingularPtrField::from(Some(proto::Error {
            message: err.to_string(),
            ..Default::default()
        }))
    }

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
