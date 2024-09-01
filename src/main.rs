mod config;
use config::Config;
use std::path::PathBuf;

use matrix_sdk::{
    config::SyncSettings,
    ruma::events::{
        room::{member::OriginalSyncRoomMemberEvent, message::RoomMessageEventContent},
        Mentions,
    },
    sync::SyncResponse,
    Client, Room, RoomState,
};

use clap::{arg, Parser};
use std::sync::OnceLock;

const VERSION: &str = env!("CARGO_PKG_VERSION");
static ROOM_FILITERS: OnceLock<Option<Vec<String>>> = OnceLock::new();

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

async fn on_room_event_message(event: OriginalSyncRoomMemberEvent, room: Room) {
    if RoomState::Joined != room.state() {
        return;
    };
    let Some(rooms) = get_rooms().await else {
        return;
    };
    let Some(room_name) = room.name() else {
        return;
    };
    if !rooms.contains(&room_name) {
        return;
    }

    let Some(display_name) = event.content.displayname else {
        return;
    };
    if display_name.len() >= 20 {
        tracing::info!("Spam! {}", event.sender);
        let reply = RoomMessageEventContent::text_plain(format!("Warning, spam, {}", event.sender))
            .set_mentions(Mentions::with_user_ids([event.sender.clone()]));
        room.send(reply.clone()).await.ok();
        if let Ok(true) = room.can_user_ban(&event.sender).await {
            tracing::info!("Ban {}", event.sender);
            room.ban_user(&event.sender, Some("UserName Spam"))
                .await
                .ok();
        }
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

    let settings = SyncSettings::default().token(response.next_batch);
    client.sync(settings).await?;
    Ok(())
}
