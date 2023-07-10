// Copyright (C) 2019-2023 Aleo Systems Inc.
// This file is part of the snarkOS library.

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:
// http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::{
    helpers::{init_bft_channels, BFTReceiver, PrimaryReceiver, PrimarySender, Storage},
    Primary,
};
use snarkos_account::Account;
use snarkvm::{
    console::account::Address,
    ledger::narwhal::BatchCertificate,
    prelude::{bail, Network, Result},
};

use parking_lot::{Mutex, RwLock};
use std::{future::Future, sync::Arc};
use tokio::task::JoinHandle;

#[derive(Clone)]
pub struct BFT<N: Network> {
    /// The primary.
    primary: Primary<N>,
    /// The batch certificate of the leader from the previous round, if one was present.
    leader_certificate: Arc<RwLock<Option<BatchCertificate<N>>>>,
    /// The spawned handles.
    handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl<N: Network> BFT<N> {
    /// Initializes a new instance of the BFT.
    pub fn new(storage: Storage<N>, account: Account<N>, dev: Option<u16>) -> Result<Self> {
        Ok(Self {
            primary: Primary::new(storage, account, dev)?,
            leader_certificate: Default::default(),
            handles: Default::default(),
        })
    }

    /// Run the BFT instance.
    pub async fn run(&mut self, primary_sender: PrimarySender<N>, primary_receiver: PrimaryReceiver<N>) -> Result<()> {
        info!("Starting the BFT instance...");
        // Initialize the BFT channels.
        let (bft_sender, bft_receiver) = init_bft_channels::<N>();
        // Run the primary instance.
        self.primary.run(primary_sender, primary_receiver, Some(bft_sender)).await?;
        // Start the BFT handlers.
        self.start_handlers(bft_receiver);
        Ok(())
    }

    /// Returns the primary.
    pub const fn primary(&self) -> &Primary<N> {
        &self.primary
    }

    /// Returns the storage.
    pub const fn storage(&self) -> &Storage<N> {
        self.primary.storage()
    }
}

impl<N: Network> BFT<N> {
    /// Returns the leader of the previous round, if one was present.
    pub fn leader(&self) -> Option<Address<N>> {
        self.leader_certificate.read().as_ref().map(|certificate| certificate.author())
    }

    /// Returns the certificate of the leader from the previous round, if one was present.
    pub const fn leader_certificate(&self) -> &Arc<RwLock<Option<BatchCertificate<N>>>> {
        &self.leader_certificate
    }

    /// Updates the leader certificate to the previous round.
    ///
    /// This method runs on every even round, by determining the leader of the previous round,
    /// and setting the leader certificate to their certificate in the previous round, if they were present.
    pub fn update_leader_certificate(&self) -> Result<()> {
        // Retrieve the current round.
        let current_round = self.storage().current_round();
        // If the current round is odd, return early.
        if current_round % 2 != 0 {
            return Ok(());
        }

        // Retrieve the previous round.
        let previous_round = current_round.saturating_sub(1);
        // Retrieve the certificates for the previous round.
        let previous_certificates = self.storage().get_certificates_for_round(previous_round);
        // If there are no previous certificates, set the previous leader certificate to 'None', and return early.
        if previous_certificates.is_empty() {
            // Set the previous leader certificate to 'None'.
            *self.leader_certificate.write() = None;
            return Ok(());
        }

        // TODO (howardwu): Determine whether to use the current round or the previous round committee.
        // Determine the leader of the previous round, using the committee of the current round.
        let leader = match self.storage().get_committee(current_round) {
            Some(committee) => committee.leader_for(current_round)?,
            None => bail!("BFT failed to retrieve the committee for the current round"),
        };
        // Find and set the leader certificate to the leader of the previous round, if they were present.
        *self.leader_certificate.write() =
            previous_certificates.into_iter().find(|certificate| certificate.author() == leader);
        Ok(())
    }
}

impl<N: Network> BFT<N> {
    /// Stores the certificate in the DAG, and attempts to commit one or more anchors.
    fn process_certificate_from_primary(&self, _certificate: BatchCertificate<N>) -> Result<()> {
        Ok(())
    }
}

impl<N: Network> BFT<N> {
    /// Starts the BFT handlers.
    fn start_handlers(&self, bft_receiver: BFTReceiver<N>) {
        let BFTReceiver { mut rx_primary_certificate } = bft_receiver;

        // Process the certificate from the primary.
        let self_ = self.clone();
        self.spawn(async move {
            while let Some(certificate) = rx_primary_certificate.recv().await {
                if let Err(e) = self_.process_certificate_from_primary(certificate) {
                    warn!("Cannot process certificate from primary - {e}");
                }
            }
        });
    }

    /// Spawns a task with the given future; it should only be used for long-running tasks.
    fn spawn<T: Future<Output = ()> + Send + 'static>(&self, future: T) {
        self.handles.lock().push(tokio::spawn(future));
    }

    /// Shuts down the BFT.
    pub async fn shut_down(&self) {
        trace!("Shutting down the BFT...");
        // Shut down the primary.
        self.primary.shut_down().await;
        // Abort the tasks.
        self.handles.lock().iter().for_each(|handle| handle.abort());
    }
}