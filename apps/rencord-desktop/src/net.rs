//! The window's network side: one runtime for the app's life, so the gateway
//! and REST calls outlive the click that started them, and one REST client per
//! session, so its rate-limit buckets are actually shared.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use rencord_api::{ApiError, DiscordApi};
use rencord_desktop::Mutation;
use rencord_gateway::{GatewayCommand, GatewayConfig, GatewayError, GatewayEvent};
use rencord_model::{Message, Snowflake, Token};
use tokio::sync::{mpsc, oneshot};

pub(crate) fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("rencord-net")
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

/// The live gateway. Dropping `commands` shuts it down. `ended` carries why it stopped.
pub(crate) struct GatewayLink {
    pub events: mpsc::Receiver<GatewayEvent>,
    pub commands: mpsc::Sender<GatewayCommand>,
    pub ended: oneshot::Receiver<Result<(), GatewayError>>,
}

/// Discord closed the socket with 4004: the token is no longer valid.
pub(crate) fn session_invalid(outcome: &Result<(), GatewayError>) -> bool {
    matches!(outcome, Err(GatewayError::Fatal(4004)))
}

pub(crate) struct Bootstrap {
    pub api: Arc<DiscordApi>,
    pub user: rencord_model::User,
    pub guilds: Vec<rencord_model::Guild>,
    pub channels: Vec<rencord_model::Channel>,
    pub gateway: GatewayLink,
    pub gateway_note: String,
    pub discord_theme: Option<String>,
    pub discord_client_theme: Option<String>,
}

pub(crate) enum BootError {
    Unauthorized,
    Other(String),
}

pub(crate) fn boot_error(err: ApiError) -> BootError {
    if err.is_unauthorized() {
        BootError::Unauthorized
    } else {
        BootError::Other(rencord_auth::redact(&err.to_string()))
    }
}

struct LiveEndpoints {
    api_base: String,
    gateway: String,
}

static ENDPOINTS: OnceLock<LiveEndpoints> = OnceLock::new();

/// Points REST and the gateway at a local stub. The default stays Discord.
pub(crate) fn set_endpoints(api_base: String, gateway: String) {
    let _ = ENDPOINTS.set(LiveEndpoints { api_base, gateway });
}

fn endpoints() -> (String, String) {
    match ENDPOINTS.get() {
        Some(live) => (live.api_base.clone(), live.gateway.clone()),
        None => (
            "https://discord.com/api/v10".into(),
            "wss://gateway.discord.gg/?v=10&encoding=json".into(),
        ),
    }
}

pub(crate) async fn fetch_bootstrap(token: String) -> Result<Bootstrap, BootError> {
    let (api_base, gateway_url) = endpoints();
    let api = Arc::new(DiscordApi::with_base(Token::new(token.clone()), api_base));
    let user = api.current_user().await.map_err(boot_error)?;
    let guilds = api.guilds().await.map_err(boot_error)?;
    let mut channels = api.private_channels().await.map_err(boot_error)?;
    if let Some(guild) = guilds.first() {
        let mut guild_channels = api.guild_channels(guild.id).await.map_err(boot_error)?;
        channels.append(&mut guild_channels);
    }
    let (gateway, gateway_note, discord_theme, discord_client_theme) =
        spawn_gateway(token, gateway_url).await;
    Ok(Bootstrap {
        api,
        user,
        guilds,
        channels,
        gateway,
        gateway_note,
        discord_theme,
        discord_client_theme,
    })
}

/// Starts the gateway and waits up to 8 s for READY, only to read the theme
/// and report status. The gateway keeps running either way.
async fn spawn_gateway(
    token: String,
    gateway_url: String,
) -> (GatewayLink, String, Option<String>, Option<String>) {
    let (events_tx, mut events) = mpsc::channel(256);
    let (commands, cmd_rx) = mpsc::channel(8);
    let (ended_tx, ended) = oneshot::channel();
    tokio::spawn(async move {
        let outcome = rencord_gateway::run(
            GatewayConfig {
                url: gateway_url,
                token,
            },
            events_tx,
            cmd_rx,
        )
        .await;
        if let Err(err) = &outcome {
            tracing::warn!(
                "gateway stopped: {}",
                rencord_auth::redact(&err.to_string())
            );
        }
        let _ = ended_tx.send(outcome);
    });
    let first = tokio::time::timeout(Duration::from_secs(8), events.recv()).await;
    let (note, theme, client_theme) = match first {
        Ok(Some(GatewayEvent::Ready {
            session_id,
            discord_theme,
            discord_client_theme,
            ..
        })) => (
            format!("ready, session {session_id}"),
            discord_theme,
            discord_client_theme,
        ),
        Ok(Some(other)) => (
            format!(
                "first event {}",
                rencord_auth::redact(&format!("{other:?}"))
            ),
            None,
            None,
        ),
        Ok(None) => ("gateway closed".into(), None, None),
        Err(_) => ("still waiting for the gateway".into(), None, None),
    };
    (
        GatewayLink {
            events,
            commands,
            ended,
        },
        note,
        theme,
        client_theme,
    )
}

/// Sends one optimistic change. A created message comes back so the caller
/// can replace its local copy.
pub(crate) async fn push_mutation(
    api: &DiscordApi,
    mutation: &Mutation,
) -> Result<Option<Message>, BootError> {
    match mutation {
        Mutation::Create {
            channel_id,
            content,
            reply_to,
            ..
        } => {
            let sent = match reply_to {
                Some(reply) => api.reply_message(*channel_id, content, *reply).await,
                None => api.send_message(*channel_id, content).await,
            };
            return sent.map(Some).map_err(boot_error);
        }
        Mutation::Edit {
            channel_id,
            message_id,
            next,
            ..
        } => {
            api.edit_message(*channel_id, *message_id, next)
                .await
                .map_err(boot_error)?;
        }
        Mutation::Delete { message } => {
            api.delete_message(message.channel_id, message.id)
                .await
                .map_err(boot_error)?;
        }
        Mutation::React {
            channel_id,
            message_id,
            emoji,
            remove,
        } => {
            let id = Snowflake::new(*message_id);
            if *remove {
                api.remove_reaction(*channel_id, id, emoji).await
            } else {
                api.add_reaction(*channel_id, id, emoji).await
            }
            .map_err(boot_error)?;
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_4004_close_invalidates_the_session() {
        assert!(session_invalid(&Err(GatewayError::Fatal(4004))));
        assert!(!session_invalid(&Err(GatewayError::Fatal(4011))));
        assert!(!session_invalid(&Err(GatewayError::Closed)));
        assert!(!session_invalid(&Ok(())));
    }
}
