mod config;
use clap::{arg, Command};
use config::Config;
use octocrab::Octocrab;
use std::path::PathBuf;
use tokio::sync::mpsc::Receiver;

use matrix_sdk::{
    config::SyncSettings, room::Room, ruma::events::room::message::RoomMessageEventContent, Client,
};

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
            let (sync_io_tx, receiver) = tokio::sync::mpsc::channel::<String>(100);
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    let notifications = octocrab.activity().notifications().list().send().await?;
                    for n in notifications {
                        println!("unread notification: {}", n.subject.title);
                        let url = n.subject.url;
                        let comment = n.subject.latest_comment_url;
                        let message = match (url, comment) {
                            (Some(url), Some(comment)) => {
                                format!("{}\n{}\n{}", n.subject.title, url, comment)
                            }
                            (Some(url), _) => format!("{}\n{}", n.subject.title, url),
                            _ => n.subject.title,
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
            login_and_sync(homeserver_url, username, password, receiver).await?;
        }
        _ => unreachable!(),
    }
    Ok(())
}

async fn login_and_sync(
    homeserver_url: String,
    username: String,
    password: String,
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
    client.sync(SyncSettings::default()).await
}
