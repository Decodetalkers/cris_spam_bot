mod config;
use clap::{arg, Command};
use config::Config;
use octocrab::Octocrab;
use std::path::PathBuf;
use tokio::sync::mpsc::Receiver;

use matrix_sdk::{
    config::SyncSettings, room::Room, ruma::events::room::message::RoomMessageEventContent, Client,
};

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

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
            let Ok(octocrab) = Octocrab::builder()
                .personal_token(config.keys.github_token)
                .build() else {
                    panic!("broken github_token");
            };
            let botid = config.botname;
            println!("bot {botid} is running");
            let homeserver_url = config.keys.homeserver;
            let username = config.keys.matrix_acount;
            let password = config.keys.matrix_passward;
            let rooms = config.keys.rooms;
            let (sync_io_tx, receiver) = tokio::sync::mpsc::channel::<String>(100);
            tokio::spawn(async move {
                let client = reqwest::Client::builder()
                    .user_agent(APP_USER_AGENT)
                    .build()?;
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    let notifications = octocrab.activity().notifications().list().send().await?;
                    for notify in notifications {
                        println!("unread notification: {}", notify.subject.title);
                        let comment = notify.subject.latest_comment_url;

                        let message = match comment {
                            Some(comment) => {
                                let text = client.get(comment.clone()).send().await?.text().await?;
                                let value: serde_json::Value = serde_json::from_str(&text)?;

                                let html_url = value["html_url"].as_str().unwrap();
                                format!("{}\n{}", notify.subject.title, html_url)
                            }
                            _ => {
                                let text = client.get(notify.url.clone()).send().await?.text().await?;
                                let value: serde_json::Value = serde_json::from_str(&text)?;
                                let html_url = value["html_url"].as_str().unwrap();
                                format!("{}\n{}", notify.subject.title, html_url)
                            }
                        };
                        let _ = sync_io_tx.send(message).await;
                    }
                    octocrab
                        .activity()
                        .notifications()
                        .mark_all_as_read(None)
                        .await?;
                }
                #[allow(unreachable_code)]
                Ok::<(), anyhow::Error>(())
            });
            login_and_sync(homeserver_url, username, password, rooms, receiver).await?;
        }
        _ => unreachable!(),
    }
    Ok(())
}

async fn login_and_sync(
    homeserver_url: String,
    username: String,
    password: String,
    allowedrooms: Option<Vec<String>>,
    mut receiver: Receiver<String>,
) -> matrix_sdk::Result<()> {
    let client = Client::builder()
        .homeserver_url(homeserver_url)
        .build()
        .await
        .unwrap_or_else(|e| panic!("{e}"));

    client
        .login_username(&username, &password)
        .initial_device_display_name("command bot")
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));

    println!("logged in as {username}");

    let clientin = client.clone();

    if let Some(allowedrooms) = allowedrooms {
        tokio::spawn(async move {
            while let Some(message) = receiver.recv().await {
                for room in clientin.rooms() {
                    if let Room::Joined(room) = room {
                        let Some(roomname) = room.name() else {
                            continue;
                        };
                        println!("the roomname is : {roomname}");
                        if !allowedrooms.contains(&roomname) {
                            continue;
                        }
                        let content = RoomMessageEventContent::text_plain(&message);
                        let _ = room.send(content, None).await;
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        });
    } else {
        tokio::spawn(async move {
            while let Some(message) = receiver.recv().await {
                for room in clientin.rooms() {
                    if let Room::Joined(room) = room {
                        let content = RoomMessageEventContent::text_plain(&message);
                        let _ = room.send(content, None).await;
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        });
    }
    client.sync(SyncSettings::default()).await
}
