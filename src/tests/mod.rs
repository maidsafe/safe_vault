// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod client;

use crate::config_handler::write_connection_info;
use crate::{Command, Config, Node};
use crossbeam_channel::Sender;
use file_per_thread_logger::{self as logger, FormatFn};
use quic_p2p::Config as NetworkConfig;
use routing::NodeConfig as RoutingConfig;
use std::io::Write;
use std::net::SocketAddr;
use tokio::task::JoinHandle;
type VaultThreads = Vec<(Sender<Command>, JoinHandle<Result<(), String>>)>;

#[allow(unused)]
#[derive(Default)]
struct Network {
    vaults: VaultThreads,
}

fn init_logging() {
    let formatter: FormatFn = |writer, record| {
        writeln!(
            writer,
            "{} [{}:{}] {}",
            record.level(),
            record.file().unwrap_or_default(),
            record.line().unwrap_or_default(),
            record.args()
        )
    };
    logger::initialize_with_formatter("", formatter);
    // logger::allow_uninitialized();
}

impl Network {
    pub async fn new(no_of_vaults: usize) -> Self {
        let path = std::path::Path::new("vaults");
        std::fs::remove_dir_all(&path).unwrap_or(()); // Delete vaults directory if it exists;
        std::fs::create_dir_all(&path).expect("Cannot create vaults directory");
        init_logging();
        logger::allow_uninitialized();
        let mut vaults = Vec::new();
        let genesis_info: SocketAddr = "127.0.0.1:12000".parse().unwrap();
        let mut node_config = Config::default();
        node_config.set_flag("local", 1);
        node_config.listen_on_loopback();
        let (command_tx, _command_rx) = crossbeam_channel::bounded(1);
        let mut genesis_config = node_config.clone();
        let handle = tokio::spawn({
            // init_logging();
            genesis_config.set_flag("first", 1);
            let path = path.join("genesis-vault");
            genesis_config.set_root_dir(&path);
            genesis_config.listen_on_loopback();

            let mut routing_config = RoutingConfig::default();
            routing_config.first = genesis_config.is_first();
            routing_config.transport_config = genesis_config.network_config().clone();

            let mut node = Node::new(&genesis_config, rand::thread_rng())
                .await
                .expect("Unable to start vault Node");
            let our_conn_info = node
                .our_connection_info()
                .expect("Could not get genesis info");
            let _ = write_connection_info(&our_conn_info).unwrap();
            node.run().await.unwrap();
            futures::future::ok(())
        });
        vaults.push((command_tx, handle));
        for i in 1..no_of_vaults {
            std::thread::sleep(std::time::Duration::from_secs(30));
            let (command_tx, _command_rx) = crossbeam_channel::bounded(1);
            let mut vault_config = node_config.clone();
            let handle = tokio::spawn({
                // init_logging();
                let vault_path = path.join(format!("vault-{}", i));
                println!("Starting new vault: {:?}", &vault_path);
                vault_config.set_root_dir(&vault_path);

                let mut network_config = NetworkConfig::default();
                let _ = network_config.hard_coded_contacts.insert(genesis_info);
                vault_config.set_network_config(network_config);
                vault_config.listen_on_loopback();

                let mut routing_config = RoutingConfig::default();
                routing_config.transport_config = vault_config.network_config().clone();

                let mut node = Node::new(&vault_config, rand::thread_rng())
                    .await
                    .expect("Unable to start vault Node");
                node.run().await.unwrap();
                futures::future::ok(())
            });
            vaults.push((command_tx, handle));
        }
        Self { vaults }
    }
}
