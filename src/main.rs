mod config;
use config::Config;
use matrix_sdk::{
    config::SyncSettings,
    ruma::{
        events::{
            room::{
                member::OriginalSyncRoomMemberEvent,
                message::{MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent},
            },
            Mentions,
        },
        OwnedUserId,
    },
    sync::SyncResponse,
    Client, Room, RoomState,
};
use tokio::sync::Mutex;

use std::path::PathBuf;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use clap::{arg, Parser};
use std::sync::OnceLock;

use std::sync::LazyLock;

const VERSION: &str = env!("CARGO_PKG_VERSION");
static ROOM_FILITERS: OnceLock<Option<Vec<String>>> = OnceLock::new();

#[derive(Debug, Clone)]
struct SpamPersion {
    id: OwnedUserId,
    count: usize,
    time: Instant,
}

impl SpamPersion {
    fn new(id: OwnedUserId) -> Self {
        Self {
            id,
            count: 1,
            time: Instant::now(),
        }
    }

    fn update(&self) -> Self {
        Self {
            id: self.id.clone(),
            count: self.count + 1,
            time: Instant::now(),
        }
    }

    fn count(&self) -> usize {
        self.count
    }

    fn time(&self) -> Instant {
        self.time
    }

    fn sender(&self) -> &OwnedUserId {
        &self.id
    }
}

static SPAMED_PERSION: LazyLock<Arc<Mutex<Option<SpamPersion>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(None)));

async fn set_spam(persion: SpamPersion) {
    let mut locked_persion = SPAMED_PERSION.lock().await;
    *locked_persion = Some(persion);
}

async fn reset_spam() {
    let mut locked_persion = SPAMED_PERSION.lock().await;
    *locked_persion = None;
}

async fn get_spam() -> Option<SpamPersion> {
    let locked_persion = SPAMED_PERSION.lock().await;
    locked_persion.clone()
}

async fn set_rooms(rooms: Option<Vec<String>>) {
    ROOM_FILITERS.set(rooms).expect("Cannot set it");
}

async fn get_rooms() -> Option<Vec<String>> {
    ROOM_FILITERS.get().unwrap_or(&None).clone()
}

#[derive(Parser, PartialEq, Eq, Debug)]
#[command(
    name ="cris_spam_bot",
    about = "a github_matrix_spam_bot",
    long_about = None,
    author = "Cris",
    version=VERSION
)]
enum MartixSpamBotCli {
    Config {
        #[arg(required = true)]
        config_path: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();

    let MartixSpamBotCli::Config { config_path } = MartixSpamBotCli::parse();

    let configsrc = Config::config_from_file(config_path);
    if configsrc.is_none() {
        panic!("error toml");
    }

    let config = configsrc.unwrap();

    let botid = config.botname;

    tracing::info!("bot {botid} is running");

    let homeserver_url = config.keys.homeserver;
    let username = config.keys.matrix_acount;
    let password = config.keys.matrix_passward;
    let rooms = config.keys.rooms;
    set_rooms(rooms).await;
    login_and_sync(homeserver_url, botid, username, password).await?;

    Ok(())
}

async fn on_room_message(event: OriginalSyncRoomMessageEvent, room: Room) {
    if RoomState::Joined != room.state() {
        return;
    };
    let Some(rooms) = get_rooms().await else {
        return;
    };
    let room_id = room.room_id();

    if !rooms.contains(&room_id.to_string()) {
        return;
    }

    let spam_check = |persion: SpamPersion, room: Room, sender: OwnedUserId| async move {
        let time = persion.time();
        let current_time = Instant::now();
        if &sender != persion.sender() {
            set_spam(SpamPersion::new(sender)).await;
            return;
        }
        if current_time - time < Duration::from_secs_f64(20.0) {
            if persion.count() > 5 {
                let reply =
                    RoomMessageEventContent::text_plain(format!("Warning, spam, {}", sender))
                        .set_mentions(Mentions::with_user_ids([sender.clone()]));
                room.send(reply.clone()).await.ok();
                tracing::info!("Ban {}", sender);
                room.ban_user(&sender, Some("Spam")).await.ok();
                reset_spam().await;
            } else {
                set_spam(persion.update()).await;
            }
        } else {
            reset_spam().await
        }
    };

    match event.content.msgtype {
        MessageType::Image(_) => {
            let spam_record = get_spam().await;
            match spam_record {
                Some(persion) => {
                    spam_check(persion, room, event.sender).await;
                }
                None => {
                    set_spam(SpamPersion::new(event.sender)).await;
                }
            }
        }
        MessageType::Text(context) => {
            if context.body.len() > 6000 {
                let reply =
                    RoomMessageEventContent::text_plain(format!("Warning, spam, {}", event.sender))
                        .set_mentions(Mentions::with_user_ids([event.sender.clone()]));
                room.send(reply.clone()).await.ok();
                tracing::info!("Ban {}", event.sender);
                room.ban_user(&event.sender, Some("Spam")).await.ok();
                reset_spam().await;
                return;
            }
            if context.body.len() < 20 {
                reset_spam().await;
                return;
            }
            let spam_record = get_spam().await;
            match spam_record {
                Some(persion) => {
                    if context.body.len() > 500 {
                        spam_check(persion, room, event.sender).await;
                    }
                }
                None => {
                    set_spam(SpamPersion::new(event.sender)).await;
                }
            }
        }
        _ => {}
    }
}

async fn on_room_event_message(event: OriginalSyncRoomMemberEvent, room: Room) {
    if RoomState::Joined != room.state() {
        return;
    };
    let Some(rooms) = get_rooms().await else {
        return;
    };
    let room_id = room.room_id();

    if !rooms.contains(&room_id.to_string()) {
        return;
    }

    let Some(display_name) = event.content.displayname else {
        return;
    };
    if display_name.len() >= 40 {
        tracing::info!("Spam! {}", event.sender);
        let reply = RoomMessageEventContent::text_plain(format!("Warning, spam, {}", event.sender))
            .set_mentions(Mentions::with_user_ids([event.sender.clone()]));
        room.send(reply.clone()).await.ok();
        tracing::info!("Ban {}", event.sender);
        room.ban_user(&event.sender, Some("UserName Spam"))
            .await
            .ok();
    }
}

async fn login_and_sync(
    homeserver_url: String,
    bot_name: String,
    username: String,
    password: String,
) -> anyhow::Result<()> {
    let client = Client::builder()
        .homeserver_url(homeserver_url)
        .build()
        .await?;

    client
        .matrix_auth()
        .login_username(&username, &password)
        .initial_device_display_name(&bot_name)
        .await?;

    let response: SyncResponse = client.sync_once(SyncSettings::default()).await?;

    client.add_event_handler(on_room_event_message);
    client.add_event_handler(on_room_message);

    let settings = SyncSettings::default().token(response.next_batch);
    client.sync(settings).await?;
    Ok(())
}
