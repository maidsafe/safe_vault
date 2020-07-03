// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod chunk_storage;
mod reading;
mod writing;

use crate::{action::Action, node::Init, rpc::Rpc as Message, utils, Config, Result};
use chunk_storage::ChunkStorage;
use reading::Reading;
use routing::{Node, SrcLocation};
use writing::Writing;

use log::{debug, error, trace};
use safe_nd::{MessageId, NodePublicId, NodeRequest, PublicId, Request, Response};
use threshold_crypto::{PublicKey, Signature};

use std::{
    cell::{Cell, RefCell},
    fmt::{self, Display, Formatter},
    rc::Rc,
};

pub(crate) struct Data {
    id: NodePublicId,
    chunk_storage: ChunkStorage,
    routing_node: Rc<RefCell<Node>>,
}

impl Data {
    pub fn new(
        id: NodePublicId,
        config: &Config,
        total_used_space: &Rc<Cell<u64>>,
        init_mode: Init,
        routing_node: Rc<RefCell<Node>>,
    ) -> Result<Self> {
        let chunk_storage = ChunkStorage::new(id.clone(), config, total_used_space, init_mode)?;

        Ok(Self {
            id,
            chunk_storage,
            routing_node,
        })
    }

    pub fn receive_msg(
        &mut self,
        src: SrcLocation,
        msg: Message,
        accumulated_signature: Option<Signature>,
    ) -> Option<Action> {
        match msg {
            Message::Request {
                request,
                requester,
                message_id,
                ..
            } => self.handle_request(src, requester, request, message_id, accumulated_signature),
            Message::Response {
                response,
                requester,
                message_id,
                proof,
                ..
            } => self.handle_response(src, response, requester, message_id, proof),
            _ => None,
        }
    }

    fn handle_request(
        &mut self,
        src: SrcLocation,
        requester: PublicId,
        request: Request,
        message_id: MessageId,
        accumulated_signature: Option<Signature>,
    ) -> Option<Action> {
        trace!(
            "{}: Received ({:?} {:?}) from src {:?} (client {:?})",
            self,
            request,
            message_id,
            src,
            requester
        );
        use NodeRequest::*;
        use Request::*;
        match request.clone() {
            Node(Read(read)) => {
                let reading = Reading::new(
                    read,
                    src,
                    requester,
                    request,
                    message_id,
                    accumulated_signature,
                    self.public_key(),
                );
                reading.get_result(&self.chunk_storage)
            }
            Node(Write(write)) => {
                let writing = Writing::new(
                    write,
                    src,
                    requester,
                    request,
                    message_id,
                    accumulated_signature,
                    self.public_key(),
                );
                writing.get_result(&mut self.chunk_storage)
            }
            _ => None,
        }
    }

    fn handle_response(
        &mut self,
        src: SrcLocation,
        response: Response,
        requester: PublicId,
        message_id: MessageId,
        proof: Option<(Request, Signature)>,
    ) -> Option<Action> {
        use Response::*;
        trace!(
            "{}: Received ({:?} {:?}) from {}",
            self,
            response,
            message_id,
            utils::get_source_name(src),
        );
        if let Some((request, signature)) = proof {
            if !matches!(requester, PublicId::Node(_))
                && self
                    .validate_section_signature(&request, &signature)
                    .is_none()
            {
                error!("Invalid section signature");
                return None;
            }
            match response {
                GetIData(result) => {
                    if matches!(requester, PublicId::Node(_)) {
                        debug!("got the duplication copy");
                        if let Ok(data) = result {
                            trace!(
                                "Got GetIData copy response for address: ({:?})",
                                data.address(),
                            );
                            self.chunk_storage.store(
                                src,
                                &data,
                                &requester,
                                message_id,
                                Some(&signature),
                                request,
                            )
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                //
                // ===== Invalid =====
                //
                ref _other => {
                    error!(
                        "{}: Should not receive {:?} as a data handler.",
                        self, response
                    );
                    None
                }
            }
        } else {
            error!("Missing section signature");
            None
        }
    }

    fn public_key(&self) -> Option<PublicKey> {
        Some(
            self.routing_node
                .borrow()
                .public_key_set()
                .ok()?
                .public_key(),
        )
    }

    fn validate_section_signature(&self, request: &Request, signature: &Signature) -> Option<()> {
        if self
            .public_key()?
            .verify(signature, &utils::serialise(request))
        {
            Some(())
        } else {
            None
        }
    }
}

impl Display for Data {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "{}", self.id.name())
    }
}
