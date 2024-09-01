mod config;
use clap::{arg, Command};
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

use std::sync::OnceLock;

static ROOM_FILITERS: OnceLock<Option<Vec<String>>> = OnceLock::new();

async fn set_rooms(rooms: Option<Vec<String>>) {
    ROOM_FILITERS.set(rooms).expect("Cannot set it");
}

async fn get_rooms() -> Option<Vec<String>> {
    ROOM_FILITERS.get().unwrap_or(&None).clone()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    const VERSION: &str = env!("CARGO_PKG_VERSION");

    let matches = Command::new("github_matrix_notifications_bot")
        .about("a github_matrix_notifications_bot")
        .version(VERSION)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .author("Cris")
        .subcommand(
            Command::new("config")
                .long_flag("config")
                .short_flag('C')
                .about("config path set")
                .arg(
                    arg!(<ConfigFile> ... "configfile").value_parser(clap::value_parser!(PathBuf)),
                ),
        )
        .get_matches();
    match matches.subcommand() {
        Some(("config", sub_matches)) => {
            let filepath = sub_matches
                .get_one::<PathBuf>("ConfigFile")
                .expect("Need a configfile");
            let configsrc = Config::config_from_file(filepath);
            if configsrc.is_none() {
                panic!("error toml");
            }
            let config = configsrc.unwrap();

            let botid = config.botname;
            println!("bot {botid} is running");
            let homeserver_url = config.keys.homeserver;
            let username = config.keys.matrix_acount;
            let password = config.keys.matrix_passward;
            let rooms = config.keys.rooms;
            set_rooms(rooms).await;
            login_and_sync(homeserver_url, username, password).await?;
        }
        _ => unreachable!(),
    }

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
        let reply = RoomMessageEventContent::text_plain(format!("Warning, spam, {}", event.sender));
        let mut methon = Mentions::new();
        methon.user_ids.insert(event.sender.clone());
        let reply = reply.set_mentions(methon);
        room.send(reply.clone()).await.ok();
        if let Ok(true) = room.can_user_ban(&event.sender).await {
            room.ban_user(&event.sender, Some("UserName Spam"))
                .await
                .ok();
        }
    }
}

async fn login_and_sync(
    homeserver_url: String,
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
        .initial_device_display_name("command bot")
        .await?;

    let response: SyncResponse = client.sync_once(SyncSettings::default()).await?;

    client.add_event_handler(on_room_event_message);

    let settings = SyncSettings::default().token(response.next_batch);
    client.sync(settings).await?;
    Ok(())
}
