use crate::proto;
use crate::proto::Raft;
use crate::raft::{Entry, Event, Message, Transport};
use crate::Error;
use crossbeam_channel::{Receiver, Sender};
use grpc::ClientStubExt;
use std::collections::HashMap;

/// A gRPC transport.
pub struct GRPC {
    /// The node channel receiver
    node_rx: Receiver<Message>,
    /// The node channel sender
    node_tx: Sender<Message>,
    /// A hash map of peer IDs and gRPC clients.
    peers: HashMap<String, proto::RaftClient>,
}

impl Transport for GRPC {
    fn receiver(&self) -> Receiver<Message> {
        self.node_rx.clone()
    }

    fn send(&self, msg: Message) -> Result<(), Error> {
        if let Some(to) = &msg.to {
            if let Some(client) = self.peers.get(to) {
                // TODO: FIXME Needs to check the response.
                client.step(grpc::RequestOptions::new(), message_to_protobuf(msg));
                Ok(())
            } else {
                Err(Error::Network(format!("Unknown Raft peer {}", to)))
            }
        } else {
            Err(Error::Network("No receiver".into()))
        }
    }
}

// TODO: revisit this
impl GRPC {
    /// Creates a new GRPC transport
    pub fn new(peers: HashMap<String, std::net::SocketAddr>) -> Result<Self, Error> {
        let (node_tx, node_rx) = crossbeam_channel::unbounded();
        let mut t = GRPC {
            peers: HashMap::new(),
            node_tx,
            node_rx,
        };
        for (id, addr) in peers.into_iter() {
            t.peers.insert(id, t.build_client(addr)?);
        }
        Ok(t)
    }

    /// Builds a gRPC client for a peer.
    pub fn build_client(&self, addr: std::net::SocketAddr) -> Result<proto::RaftClient, Error> {
        Ok(proto::RaftClient::new_plain(
            &addr.ip().to_string(),
            addr.port(),
            grpc::ClientConf::new(),
        )?)
    }

    /// Builds a gRPC service for a local server.
    pub fn build_service(&self) -> Result<impl proto::Raft, Error> {
        Ok(GRPCService {
            local: self.node_tx.clone(),
        })
    }
}

/// A gRPC service for a local server.
struct GRPCService {
    local: Sender<Message>,
}

impl proto::Raft for GRPCService {
    fn step(
        &self,
        _: grpc::RequestOptions,
        pb: proto::Message,
    ) -> grpc::SingleResponse<proto::Success> {
        self.local.send(message_from_protobuf(pb).unwrap()).unwrap();
        grpc::SingleResponse::completed(proto::Success::new())
    }
}

/// Converts a Protobuf message to a `Message`.
fn message_from_protobuf(pb: proto::Message) -> Result<Message, Error> {
    Ok(Message {
        term: pb.term,
        from: Some(pb.from),
        to: Some(pb.to),
        event: match pb.event {
            Some(proto::Message_oneof_event::heartbeat(e)) => Event::Heartbeat {
                commit_index: e.commit_index,
                commit_term: e.commit_term,
            },
            Some(proto::Message_oneof_event::confirm_leader(e)) => Event::ConfirmLeader {
                commit_index: e.commit_index,
                has_committed: e.has_committed,
            },
            Some(proto::Message_oneof_event::solicit_vote(e)) => Event::SolicitVote {
                last_index: e.last_index,
                last_term: e.last_term,
            },
            Some(proto::Message_oneof_event::grant_vote(_)) => Event::GrantVote,
            Some(proto::Message_oneof_event::read_state(e)) => Event::ReadState {
                call_id: e.call_id,
                command: e.command,
            },
            Some(proto::Message_oneof_event::mutate_state(e)) => Event::MutateState {
                call_id: e.call_id,
                command: e.command,
            },
            Some(proto::Message_oneof_event::respond_state(e)) => Event::RespondState {
                call_id: e.call_id,
                response: e.response,
            },
            Some(proto::Message_oneof_event::respond_error(e)) => Event::RespondError {
                call_id: e.call_id,
                error: e.error,
            },
            Some(proto::Message_oneof_event::replicate_entries(e)) => Event::ReplicateEntries {
                base_index: e.base_index,
                base_term: e.base_term,
                entries: e
                    .entries
                    .to_vec()
                    .into_iter()
                    .map(|entry| Entry {
                        term: entry.term,
                        command: if entry.command.is_empty() {
                            None
                        } else {
                            Some(entry.command)
                        },
                    })
                    .collect(),
            },
            Some(proto::Message_oneof_event::accept_entries(e)) => Event::AcceptEntries {
                last_index: e.last_index,
            },
            Some(proto::Message_oneof_event::reject_entries(_)) => Event::RejectEntries,
            None => return Err(Error::Network("No event found in protobuf message".into())),
        },
    })
}

/// Converts a `Message` to its Protobuf representation.
fn message_to_protobuf(msg: Message) -> proto::Message {
    proto::Message {
        term: msg.term,
        from: msg.from.unwrap(),
        to: msg.to.unwrap(),
        event: Some(match msg.event {
            Event::Heartbeat {
                commit_index,
                commit_term,
            } => proto::Message_oneof_event::heartbeat(proto::Heartbeat {
                commit_index,
                commit_term,
                ..Default::default()
            }),
            Event::ConfirmLeader {
                commit_index,
                has_committed,
            } => proto::Message_oneof_event::confirm_leader(proto::ConfirmLeader {
                commit_index,
                has_committed,
                ..Default::default()
            }),
            Event::SolicitVote {
                last_index,
                last_term,
            } => proto::Message_oneof_event::solicit_vote(proto::SolicitVote {
                last_index,
                last_term,
                ..Default::default()
            }),
            Event::GrantVote => proto::Message_oneof_event::grant_vote(proto::GrantVote::new()),
            Event::ReadState { call_id, command } => {
                proto::Message_oneof_event::read_state(proto::ReadState {
                    call_id,
                    command,
                    ..Default::default()
                })
            }
            Event::MutateState { call_id, command } => {
                proto::Message_oneof_event::mutate_state(proto::MutateState {
                    call_id,
                    command,
                    ..Default::default()
                })
            }
            Event::RespondState { call_id, response } => {
                proto::Message_oneof_event::respond_state(proto::RespondState {
                    call_id,
                    response,
                    ..Default::default()
                })
            }
            Event::RespondError { call_id, error } => {
                proto::Message_oneof_event::respond_error(proto::RespondError {
                    call_id,
                    error,
                    ..Default::default()
                })
            }
            Event::ReplicateEntries {
                base_index,
                base_term,
                entries,
            } => proto::Message_oneof_event::replicate_entries(proto::ReplicateEntries {
                base_index,
                base_term,
                entries: protobuf::RepeatedField::from_vec(
                    entries
                        .into_iter()
                        .map(|entry| proto::Entry {
                            term: entry.term,
                            command: if let Some(bytes) = entry.command {
                                bytes
                            } else {
                                vec![]
                            },
                            ..Default::default()
                        })
                        .collect(),
                ),
                ..Default::default()
            }),
            Event::AcceptEntries { last_index } => {
                proto::Message_oneof_event::accept_entries(proto::AcceptEntries {
                    last_index,
                    ..Default::default()
                })
            }
            Event::RejectEntries => {
                proto::Message_oneof_event::reject_entries(proto::RejectEntries::new())
            }
        }),
        ..Default::default()
    }
}
