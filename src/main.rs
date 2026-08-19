mod app;
mod auth;
mod config;
mod db;
mod providers;
mod secrets;
mod service;
mod web;

use anyhow::{Context, Result};
use auth::hash_password;
use clap::{Args, Parser, Subcommand};
use config::{default_data_dir, AppConfig, DEFAULT_LISTEN};
use rand::Rng;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "wakebridge",
    version,
    about = "Multi-site Wake-on-LAN management"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Run(RunArgs),
    #[command(subcommand)]
    User(UserCommand),
    #[command(subcommand)]
    Service(ServiceCommand),
}

#[derive(Args)]
struct RunArgs {
    #[arg(long, default_value = DEFAULT_LISTEN)]
    listen: String,
    #[arg(long)]
    data_dir: Option<PathBuf>,
    #[arg(long)]
    service: bool,
}

#[derive(Subcommand)]
enum UserCommand {
    Create(UserCreateArgs),
    ResetPassword(UserResetArgs),
    List(UserListArgs),
}

#[derive(Args)]
struct UserCreateArgs {
    #[arg(long)]
    username: String,
    #[arg(long, default_value = "operator")]
    role: String,
    #[arg(long)]
    password: Option<String>,
    #[arg(long)]
    data_dir: Option<PathBuf>,
}

#[derive(Args)]
struct UserResetArgs {
    #[arg(long)]
    username: String,
    #[arg(long)]
    password: Option<String>,
    #[arg(long)]
    data_dir: Option<PathBuf>,
}

#[derive(Args)]
struct UserListArgs {
    #[arg(long)]
    data_dir: Option<PathBuf>,
}

#[derive(Subcommand)]
enum ServiceCommand {
    Install,
    Uninstall,
    Start,
    Stop,
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Run(args) => run_command(args).await,
        Command::User(command) => user_command(command).await,
        Command::Service(command) => service_command(command),
    }
}

async fn run_command(args: RunArgs) -> Result<()> {
    let config = AppConfig::new(&args.listen, args.data_dir, args.service)?;
    if args.service {
        return service::run_service();
    }
    let state = app::AppState::initialize(config).await?;
    web::serve_foreground(state, async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl+C handler");
    })
    .await
}

async fn user_command(command: UserCommand) -> Result<()> {
    let (data_dir, action) = match command {
        UserCommand::Create(args) => {
            let password = args.password.unwrap_or_else(generate_password);
            let data_dir = args.data_dir.unwrap_or_else(default_data_dir);
            let config = AppConfig::new(DEFAULT_LISTEN, Some(data_dir.clone()), false)?;
            let state = app::AppState::initialize(config).await?;
            let role = auth::Role::parse(&args.role).context("role must be admin or operator")?;
            let password_hash = hash_password(&password)?;
            let id = state
                .db
                .insert_user(&args.username, &password_hash, role.as_str())
                .await
                .context("create user")?;
            println!(
                "Created user {} (id={}, role={})",
                args.username,
                id,
                role.as_str()
            );
            println!("Generated password (show once): {password}");
            return Ok(());
        }
        UserCommand::ResetPassword(args) => {
            let password = args.password.unwrap_or_else(generate_password);
            (
                args.data_dir.unwrap_or_else(default_data_dir),
                (args.username, password),
            )
        }
        UserCommand::List(args) => {
            let data_dir = args.data_dir.unwrap_or_else(default_data_dir);
            let config = AppConfig::new(DEFAULT_LISTEN, Some(data_dir), false)?;
            let state = app::AppState::initialize(config).await?;
            for user in state.db.list_users().await? {
                println!(
                    "{}\t{}\t{}",
                    user.username,
                    user.role,
                    if user.enabled { "enabled" } else { "disabled" }
                );
            }
            return Ok(());
        }
    };
    let config = AppConfig::new(DEFAULT_LISTEN, Some(data_dir), false)?;
    let state = app::AppState::initialize(config).await?;
    let password_hash = hash_password(&action.1)?;
    if !state.db.update_password(&action.0, &password_hash).await? {
        anyhow::bail!("user does not exist: {}", action.0);
    }
    println!("Reset password for {}", action.0);
    println!("Generated password (show once): {}", action.1);
    Ok(())
}

fn service_command(command: ServiceCommand) -> Result<()> {
    match command {
        ServiceCommand::Install => service::install(),
        ServiceCommand::Uninstall => service::uninstall(),
        ServiceCommand::Start => service::start(),
        ServiceCommand::Stop => service::stop(),
        ServiceCommand::Status => service::status(),
    }
}

fn generate_password() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789!@#$%^*-_+=";
    let mut rng = rand::thread_rng();
    (0..24)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect()
}
