use grpc::ClientStubExt;

use proto::StoreService;

use crate::proto;
use crate::proto::Field_oneof_value;
use crate::serializer::deserialize;
use crate::sql::types::{Row, Value};
use crate::Error;

/// A Store client
pub struct Client {
    client: proto::StoreServiceClient,
}

impl Client {
    /// Creates a new client
    pub fn new(host: &str, port: u16) -> Result<Self, Error> {
        Ok(Self {
            client: proto::StoreServiceClient::new_plain(host, port, grpc::ClientConf::new())?,
        })
    }

    /// Runs a query
    pub fn query(&self, query: &str) -> Result<ResultSet, Error> {
        let (metadata, iter) = self
            .client
            .query(
                grpc::RequestOptions::new(),
                proto::QueryRequest {
                    query: query.to_owned(),
                    ..Default::default()
                },
            )
            .wait()?;
        ResultSet::from_grpc(metadata, iter)
    }

    /// Fetches the table schema as SQL
    pub fn get_table(&self, table: &str) -> Result<String, Error> {
        let (_, resp, _) = self
            .client
            .get_table(
                grpc::RequestOptions::new(),
                proto::GetTableRequest {
                    name: table.to_string(),
                    ..Default::default()
                },
            )
            .wait()?;
        Ok(resp.sql)
    }

    /// Checks server status
    pub fn status(&self) -> Result<Status, Error> {
        let (_, resp, _) = self
            .client
            .status(grpc::RequestOptions::new(), proto::StatusRequest::new())
            .wait()?;
        Ok(Status {
            id: resp.id,
            version: resp.version,
        })
    }
}

pub struct ResultSet {
    columns: Vec<String>,
    rows: Box<dyn Iterator<Item = Result<proto::Row, grpc::Error>>>,
}

impl Iterator for ResultSet {
    type Item = Result<Row, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(
            self.rows
                .next()?
                .map(Self::row_from_protobuf)
                .map_err(|e| e.into()),
        )
    }
}

impl ResultSet {
    fn from_grpc(
        metadata: grpc::Metadata,
        rows: Box<dyn std::iter::Iterator<Item = Result<proto::Row, grpc::Error>>>,
    ) -> Result<Self, Error> {
        let columns = deserialize(
            metadata
                .get("columns")
                .map(|c| c.to_vec())
                .ok_or_else(|| Error::Network("Columns not found in gRPC result".into()))?,
        )?;

        Ok(Self { columns, rows })
    }

    pub fn columns(&self) -> Vec<String> {
        self.columns.clone()
    }

    fn row_from_protobuf(proto_row: proto::Row) -> Row {
        proto_row
            .field
            .into_iter()
            .map(Self::value_from_protobuf)
            .collect()
    }

    fn value_from_protobuf(field: proto::Field) -> Value {
        match field.value {
            None => Value::Null,
            Some(Field_oneof_value::boolean(b)) => Value::Boolean(b),
            Some(Field_oneof_value::integer(i)) => Value::Integer(i),
            Some(Field_oneof_value::float(f)) => Value::Float(f),
            Some(Field_oneof_value::string(s)) => Value::String(s),
        }
    }
}

/// Server status
pub struct Status {
    pub id: String,
    pub version: String,
}
