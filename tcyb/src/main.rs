mod api;
mod auth;
mod channel;
mod chat;
mod eventsub;
mod irc;
mod paths;
mod profiling;
mod settings;
mod store;
mod yomiage;
use anyhow::Result;
use clap::{Parser, Subcommand};
use settings::Settings;
use std::path::PathBuf;

#[derive(Parser)]
struct Cli {
    #[arg(short, long)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    ReadChat {},
    AuthCode {},
    BanBots {},
    RefreshToken {},
    ShowChatters {
        /// 指定した秒数ごとに取得を繰り返す（省略時は 1 回取得して終了）
        #[arg(long, value_name = "SECS", value_parser = clap::value_parser!(u64).range(1..))]
        interval: Option<u64>,
    },
    ShowUser {
        username: String,
    },
    ShowFollowings {
        username: String,
    },
}

/// `run` を Ctrl+C 受信まで走らせる。先に完了した方の結果を返す。
async fn run_until_ctrl_c(run: impl std::future::Future<Output = Result<()>>) -> Result<()> {
    tokio::select! {
        res = run => res,
        sig = tokio::signal::ctrl_c() => {
            sig?;
            log::warn!("Ctrl+C received, shutting down");
            Ok(())
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let _profile = profiling::init();
    let args = Cli::parse();
    let app_paths = paths::app_paths()?;

    let settings: Settings = {
        let _span = tracing::info_span!("config_build").entered();
        if !app_paths.config_file.exists() && args.config.is_none() {
            settings::scaffold_config(&app_paths.config_file)?;
            println!(
                "設定ファイルを作成しました: {}",
                app_paths.config_file.display()
            );
            println!("client_id / client_secret などを記入してから再実行してください。");
            return Ok(());
        }
        settings::load(
            &app_paths.config_file,
            args.config.as_deref(),
            &app_paths.db_dir,
        )?
    };

    {
        let _span = tracing::info_span!("logger_init").entered();
        simple_logger::SimpleLogger::new()
            .env()
            .with_local_timestamps()
            .init()?;
    }

    match &args.command {
        Some(Commands::ReadChat {}) => {
            run_until_ctrl_c(yomiage::yomiage(&settings)).await?;
        }
        Some(Commands::AuthCode {}) => {
            auth::auth_code_grant(
                &settings.listen_address,
                &settings.db_dir,
                &settings.db_name,
                &settings.client_id,
                &settings.client_secret,
            )
            .await?;
        }
        Some(Commands::BanBots {}) => {
            channel::ban_bots(
                &settings.db_dir,
                &settings.db_name,
                &settings.username,
                &settings.client_id,
            )
            .await?;
        }
        Some(Commands::RefreshToken {}) => {
            auth::refresh_token_grant(
                &settings.db_dir,
                &settings.db_name,
                &settings.client_id,
                &settings.client_secret,
            )
            .await?;
        }
        Some(Commands::ShowChatters { interval }) => {
            let interval = interval.map(std::time::Duration::from_secs);
            let run = chat::chatters(
                &settings.db_dir,
                &settings.db_name,
                &settings.channel,
                &settings.username,
                &settings.client_id,
                &settings.client_secret,
                interval,
            );
            if interval.is_none() {
                run.await?;
            } else {
                run_until_ctrl_c(run).await?;
            }
        }
        Some(Commands::ShowUser { username }) => {
            chat::show_user_info(
                &settings.db_dir,
                &settings.db_name,
                username,
                &settings.client_id,
                &settings.client_secret,
            )
            .await?;
        }
        Some(Commands::ShowFollowings { username }) => {
            channel::show_following_info(
                &settings.db_dir,
                &settings.db_name,
                username,
                &settings.client_id,
                &settings.client_secret,
            )
            .await?;
        }
        None => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_interval(args: &[&str]) -> Option<u64> {
        match Cli::try_parse_from(args).unwrap().command {
            Some(Commands::ShowChatters { interval }) => interval,
            _ => panic!("expected the show-chatters subcommand"),
        }
    }

    #[test]
    fn show_chatters_defaults_to_one_shot() {
        assert_eq!(parse_interval(&["tcyb", "show-chatters"]), None);
    }

    #[test]
    fn show_chatters_takes_interval_in_seconds() {
        assert_eq!(
            parse_interval(&["tcyb", "show-chatters", "--interval", "60"]),
            Some(60)
        );
    }

    #[test]
    fn show_chatters_rejects_zero_interval() {
        assert!(Cli::try_parse_from(["tcyb", "show-chatters", "--interval", "0"]).is_err());
    }

    #[test]
    fn show_chatters_rejects_non_numeric_interval() {
        assert!(Cli::try_parse_from(["tcyb", "show-chatters", "--interval", "1m"]).is_err());
    }

    #[test]
    fn cli_definition_is_internally_consistent() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
