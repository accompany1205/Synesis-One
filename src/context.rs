use dashmap::DashMap;
use serde::{Serialize, Deserialize};
use solana_client::{rpc_client::RpcClient, rpc_response::{RpcSignatureResult, ReceivedSignatureResult, RpcResponseContext}};
use solana_rpc::{rpc_subscription_tracker::{SubscriptionId, SubscriptionParams, SignatureSubscriptionParams}, rpc_subscriptions::RpcNotification};
use solana_sdk::{commitment_config::{CommitmentConfig, CommitmentLevel}, signature::Signature};
use tokio::sync::broadcast;
use std::{
    collections::HashMap,
    sync::{atomic::AtomicU64, Arc, RwLock}, time::Instant,
};

pub struct BlockInformation {
    pub block_hash: RwLock<String>,
    pub block_height: AtomicU64,
    pub slot: AtomicU64,
    pub confirmation_level: CommitmentLevel,
}

impl BlockInformation {
    pub fn new(rpc_client: Arc<RpcClient>, commitment: CommitmentLevel) -> Self {
        let slot = rpc_client
            .get_slot_with_commitment(CommitmentConfig { commitment })
            .unwrap();

        let (blockhash, blockheight) = rpc_client
            .get_latest_blockhash_with_commitment(CommitmentConfig { commitment })
            .unwrap();

        BlockInformation {
            block_hash: RwLock::new(blockhash.to_string()),
            block_height: AtomicU64::new(blockheight),
            slot: AtomicU64::new(slot),
            confirmation_level: commitment,
        }
    }
}

pub struct LiteRpcContext {
    pub signature_status: RwLock<HashMap<String, Option<CommitmentLevel>>>,
    pub finalized_block_info: BlockInformation,
    pub confirmed_block_info: BlockInformation,
}

impl LiteRpcContext {
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        LiteRpcContext {
            signature_status: RwLock::new(HashMap::new()),
            confirmed_block_info: BlockInformation::new(
                rpc_client.clone(),
                CommitmentLevel::Confirmed,
            ),
            finalized_block_info: BlockInformation::new(rpc_client, CommitmentLevel::Finalized),
        }
    }
}

pub struct SignatureNotification {
    pub signature : Signature,
    pub commitment : CommitmentLevel,
    pub slot : u64,
    pub error : Option<String>,
}

pub enum NotificationType {
    Signature(SignatureNotification),
    Slot(u64),
}


#[derive(Debug, Serialize)]
struct NotificationParams<T> {
    result: T,
    subscription: SubscriptionId,
}

#[derive(Debug, Serialize)]
struct Notification<T> {
    jsonrpc: Option<jsonrpc_core::Version>,
    method: &'static str,
    params: NotificationParams<T>,
}

pub struct LiteRpcSubsrciptionControl {
    broadcast_sender: broadcast::Sender<LiteRpcNotification>,
    notification_reciever : crossbeam_channel::Receiver<NotificationType>,
    subscriptions : DashMap<SubscriptionParams, SubscriptionId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Response<T> {
    pub context: RpcResponseContext,
    pub value: T,
}


#[derive(Debug, Clone, PartialEq)]
struct RpcNotificationResponse<T> {
    context: RpcNotificationContext,
    value: T,
}

impl<T> From<RpcNotificationResponse<T>> for Response<T> {
    fn from(notification: RpcNotificationResponse<T>) -> Self {
        let RpcNotificationResponse {
            context: RpcNotificationContext { slot },
            value,
        } = notification;
        Self {
            context: RpcResponseContext {
                slot,
                api_version: None,
            },
            value,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RpcNotificationContext {
    slot: u64,
}


#[derive(Debug, Clone)]
pub struct LiteRpcNotification {
    pub subscription_id: SubscriptionId,
    pub is_final: bool,
    pub json: String,
    pub created_at: Instant,
}


impl LiteRpcSubsrciptionControl {
    pub fn new(
        broadcast_sender: broadcast::Sender<LiteRpcNotification>,
        notification_reciever : crossbeam_channel::Receiver<NotificationType>,
    ) -> Self {
        Self { broadcast_sender, 
            notification_reciever,
            subscriptions : DashMap::new(),
        }
    }

    pub fn start_broadcasting(&self) {
        loop {
            let notification = self.notification_reciever.recv();
            match notification {
                Ok(notification_type) => {
                    let rpc_notification = match notification_type {
                        NotificationType::Signature(data) => {
                            let signature_params = SignatureSubscriptionParams {
                                commitment: CommitmentConfig {
                                    commitment: data.commitment,
                                },
                                signature: data.signature,
                                enable_received_notification: false,
                            };
                            
                            let param = SubscriptionParams::Signature(signature_params);

                            match self.subscriptions.entry(param) {
                                dashmap::mapref::entry::Entry::Occupied(x) => {
                                    let subscription_id = *x.get();
                                    let slot = data.slot;
                                    let value = Response::from(RpcNotificationResponse {
                                        context: RpcNotificationContext { slot },
                                        value: RpcSignatureResult::ReceivedSignature(
                                            ReceivedSignatureResult::ReceivedSignature,
                                        ),
                                    });

                                    let notification = Notification {
                                        jsonrpc: Some(jsonrpc_core::Version::V2),
                                        method: &"signatureSubscription",
                                        params: NotificationParams {
                                            result: value,
                                            subscription: subscription_id,
                                        },
                                    };
                                    let json = serde_json::to_string(&notification).unwrap();
                                    Some( LiteRpcNotification{
                                        subscription_id : *x.get(),
                                        created_at : Instant::now(),
                                        is_final: false,
                                        json,
                                    } )
                                },
                                dashmap::mapref::entry::Entry::Vacant(x) => {
                                    None
                                }
                            }                            
                        },
                        NotificationType::Slot(slot) => {
                            // SubscriptionId 0 will be used for slots
                            None
                        }
                    };
                    if let Some(rpc_notification) = rpc_notification {
                        self.broadcast_sender.send(rpc_notification).unwrap();
                    }
                },
                Err(e) => {
                    println!("LiteRpcSubsrciptionControl notification channel recieved error {}", e.to_string());
                }
            }
        }
    }
}
