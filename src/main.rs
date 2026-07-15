//! clap parsing and dispatch only — all logic lives in the library.

use clap::error::ErrorKind as ClapErrorKind;
use clap::{ArgGroup, Parser, Subcommand};
use serde_json::{json, Value};

use swb::emit::{self, SwbError};
use swb::{ble, cloud, config};

#[derive(Parser)]
#[command(
    name = "swb",
    version,
    about = "UNIX-philosophy CLI for SwitchBot devices"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// BLE passive scan; emits JSONL on stdout
    #[command(group(ArgGroup::new("mode").required(true).args(["follow", "once"])))]
    Scan {
        /// Scan continuously
        #[arg(long)]
        follow: bool,
        /// Single collection pass (one line per device), then exit
        #[arg(long)]
        once: bool,
        /// Collection window in seconds for --once (config default_timeout, else 10)
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// List models with BLE decode support (static info)
    Models,
    /// Cloud: list devices
    Devices,
    /// Cloud: fetch device status
    Status {
        /// Device ID or config alias
        device: String,
    },
    /// Cloud: send a control command
    Cmd {
        /// Device ID or config alias
        device: String,
        /// Command name, e.g. turnOn
        command: String,
        /// Command parameter as JSON (plain strings allowed); default "default"
        #[arg(long)]
        param: Option<String>,
    },
}

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e)
            if matches!(
                e.kind(),
                ClapErrorKind::DisplayHelp | ClapErrorKind::DisplayVersion
            ) =>
        {
            let _ = e.print();
            std::process::exit(0);
        }
        Err(e) => exit_with(SwbError::usage(e.to_string())),
    };

    if let Err(e) = run(cli) {
        exit_with(e);
    }
}

fn exit_with(e: SwbError) -> ! {
    eprintln!("{}", e.to_json());
    std::process::exit(e.kind.exit_code());
}

fn run(cli: Cli) -> Result<(), SwbError> {
    match cli.command {
        Command::Scan {
            follow,
            once: _,
            timeout,
        } => {
            let cfg = config::load()?;
            let timeout = timeout.or(cfg.default_timeout).unwrap_or(10);
            ble::run_scan(follow, timeout)
        }
        Command::Models => {
            let models: Vec<Value> = ble::parse::MODELS
                .iter()
                .map(|m| {
                    json!({
                        "model": m.model,
                        "tag": m.tag.to_string(),
                        "values": m.values,
                    })
                })
                .collect();
            emit::print_json(&models);
            Ok(())
        }
        Command::Devices => {
            let cfg = config::load()?;
            let body = client(&cfg)?.get("/v1.1/devices")?;
            emit::print_json(&body);
            Ok(())
        }
        Command::Status { device } => {
            let cfg = config::load()?;
            let id = cfg.resolve_device(&device);
            let mut body = client(&cfg)?.get(&format!("/v1.1/devices/{id}/status"))?;
            attach_device_id(&mut body, &id);
            emit::print_json(&body);
            Ok(())
        }
        Command::Cmd {
            device,
            command,
            param,
        } => {
            let cfg = config::load()?;
            let id = cfg.resolve_device(&device);
            let parameter = match &param {
                // JSON if it parses, otherwise treat the raw text as a string
                Some(p) => serde_json::from_str(p).unwrap_or_else(|_| Value::String(p.clone())),
                None => Value::String("default".to_string()),
            };
            let payload = json!({
                "command": command,
                "parameter": parameter,
                "commandType": "command",
            });
            let mut body = client(&cfg)?.post(&format!("/v1.1/devices/{id}/commands"), &payload)?;
            attach_device_id(&mut body, &id);
            emit::print_json(&body);
            Ok(())
        }
    }
}

fn client(cfg: &config::Config) -> Result<cloud::Client, SwbError> {
    let (token, secret) =
        cfg.credentials(env_var("SWITCHBOT_TOKEN"), env_var("SWITCHBOT_SECRET"))?;
    Ok(cloud::Client::new(token, secret))
}

fn env_var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn attach_device_id(body: &mut Value, id: &str) {
    if let Value::Object(map) = body {
        map.insert("device_id".to_string(), Value::String(id.to_string()));
    }
}
