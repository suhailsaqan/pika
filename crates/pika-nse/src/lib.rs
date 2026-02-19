mod mdk_support;

use mdk_core::prelude::MessageProcessingResult;
use nostr::{Event, Kind};

uniffi::setup_scaffolding!();

#[derive(uniffi::Record)]
pub struct PushNotificationContent {
    pub chat_id: String,
    pub sender_pubkey: String,
    pub sender_name: String,
    pub sender_picture_url: Option<String>,
    pub content: String,
    pub is_group: bool,
    pub group_name: Option<String>,
}

#[derive(uniffi::Enum)]
pub enum PushNotificationResult {
    /// Decrypted successfully — show the notification.
    Content { content: PushNotificationContent },
    /// Incoming call invite — show call notification.
    CallInvite {
        chat_id: String,
        call_id: String,
        caller_name: String,
        caller_picture_url: Option<String>,
    },
    /// Recognised but should not alert (self-message, call signal, etc.).
    Suppress,
}

#[derive(serde::Deserialize)]
struct CallProbe {
    #[serde(rename = "type")]
    msg_type: String,
    call_id: String,
}

#[uniffi::export]
pub fn decrypt_push_notification(
    data_dir: String,
    nsec: String,
    event_json: String,
    keychain_group: String,
) -> Option<PushNotificationResult> {
    pika_tls::init_rustls_crypto_provider();

    let keys = nostr::Keys::parse(&nsec).ok()?;
    let pubkey = keys.public_key();

    let mdk = mdk_support::open_mdk(&data_dir, &pubkey, &keychain_group).ok()?;

    let event: Event = serde_json::from_str(&event_json).ok()?;

    let result = mdk.process_message(&event).ok()?;

    let msg = match result {
        MessageProcessingResult::ApplicationMessage(msg) => msg,
        _ => return None,
    };

    // Don't notify for self-messages.
    if msg.pubkey == pubkey {
        return Some(PushNotificationResult::Suppress);
    }

    let group = mdk.get_group(&msg.mls_group_id).ok()??;
    let chat_id = hex::encode(group.nostr_group_id);

    match msg.kind {
        Kind::ChatMessage => {
            // For chat messages, if decryption failed, suppress the notification.
            if msg.content.is_empty() {
                return Some(PushNotificationResult::Suppress);
            }

            let all_groups = mdk.get_groups().ok()?;
            let group_info = all_groups
                .iter()
                .find(|g| g.mls_group_id == msg.mls_group_id);

            let group_name = group_info.and_then(|g| {
                if g.name != "DM" && !g.name.is_empty() {
                    Some(g.name.clone())
                } else {
                    None
                }
            });

            let members = mdk.get_members(&msg.mls_group_id).unwrap_or_default();
            let other_count = members.iter().filter(|p| *p != &pubkey).count();
            let is_group = other_count > 1 || (group_name.is_some() && other_count > 0);

            let sender_hex = msg.pubkey.to_hex();
            let (sender_name, sender_picture_url) = resolve_sender_profile(&data_dir, &sender_hex);

            Some(PushNotificationResult::Content {
                content: PushNotificationContent {
                    chat_id,
                    sender_pubkey: sender_hex,
                    sender_name,
                    sender_picture_url,
                    content: msg.content,
                    is_group,
                    group_name,
                },
            })
        }
        Kind::Custom(10) => {
            let probe: CallProbe = serde_json::from_str(&msg.content).ok()?;
            if probe.msg_type != "call.invite" {
                return Some(PushNotificationResult::Suppress);
            }
            let sender_hex = msg.pubkey.to_hex();
            let (caller_name, caller_picture_url) = resolve_sender_profile(&data_dir, &sender_hex);
            Some(PushNotificationResult::CallInvite {
                chat_id,
                call_id: probe.call_id,
                caller_name,
                caller_picture_url,
            })
        }
        _ => Some(PushNotificationResult::Suppress),
    }
}

/// Look up display name and picture URL from the SQLite profile cache.
fn resolve_sender_profile(data_dir: &str, pubkey_hex: &str) -> (String, Option<String>) {
    let fallback = (format!("{}...", &pubkey_hex[..8]), None);

    let db_path = std::path::Path::new(data_dir).join("profiles.sqlite3");
    let conn = match rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(_) => return fallback,
    };

    let row: Option<(Option<String>, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT metadata->>'display_name', metadata->>'name', metadata->>'picture'
             FROM profiles WHERE pubkey = ?1",
            [pubkey_hex],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok();

    let Some((display_name, name_field, picture)) = row else {
        return fallback;
    };

    let name = display_name
        .filter(|s| !s.is_empty())
        .or(name_field.filter(|s| !s.is_empty()))
        .unwrap_or_else(|| format!("{}...", &pubkey_hex[..8]));

    let picture_url = picture.filter(|s| !s.is_empty()).map(|url| {
        // Prefer locally cached profile picture if available.
        let cached = std::path::Path::new(data_dir)
            .join("profile_pics")
            .join(pubkey_hex);
        if cached.exists() {
            format!("file://{}", cached.display())
        } else {
            url
        }
    });

    (name, picture_url)
}
