use std::collections::HashMap;
use std::sync::Mutex;

use plotweb_common::{BetaFeedback, BetaFeedbackReply};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    NewFeedback(BetaFeedback),
    NewReply {
        feedback_id: String,
        reply: BetaFeedbackReply,
    },
    FeedbackResolved {
        feedback_id: String,
        resolved: bool,
    },
    FeedbackDeleted {
        feedback_id: String,
    },
}

pub struct FeedbackBroadcaster {
    channels: Mutex<HashMap<String, broadcast::Sender<String>>>,
}

impl FeedbackBroadcaster {
    pub fn new() -> Self {
        Self {
            channels: Mutex::new(HashMap::new()),
        }
    }

    pub fn subscribe(&self, book_id: &str) -> broadcast::Receiver<String> {
        let mut channels = self.channels.lock().unwrap();
        let sender = channels
            .entry(book_id.to_string())
            .or_insert_with(|| broadcast::channel(64).0);
        sender.subscribe()
    }

    pub fn broadcast(&self, book_id: &str, msg: &WsMessage) {
        let channels = self.channels.lock().unwrap();
        if let Some(sender) = channels.get(book_id) {
            if let Ok(json) = serde_json::to_string(msg) {
                // Ignore send errors (no subscribers)
                let _ = sender.send(json);
            }
        }
    }
}
