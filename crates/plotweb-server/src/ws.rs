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
        let mut channels = self.channels.lock().unwrap_or_else(|e| e.into_inner());
        let sender = channels
            .entry(book_id.to_string())
            .or_insert_with(|| broadcast::channel(64).0);
        sender.subscribe()
    }

    pub fn broadcast(&self, book_id: &str, msg: &WsMessage) {
        // Serialize before taking the lock so a serialization failure (or the
        // work itself) doesn't hold the mutex.
        let Ok(json) = serde_json::to_string(msg) else {
            return;
        };
        let mut channels = self.channels.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(sender) = channels.get(book_id) {
            // send() errors only when there are no receivers; drop the entry.
            if sender.send(json).is_err() {
                channels.remove(book_id);
            }
        }
    }

    /// Remove a book's channel if it has no remaining receivers. Called when a
    /// websocket connection closes so the map doesn't grow unboundedly.
    pub fn cleanup(&self, book_id: &str) {
        let mut channels = self.channels.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = channels.get(book_id) {
            if s.receiver_count() == 0 {
                channels.remove(book_id);
            }
        }
    }
}
