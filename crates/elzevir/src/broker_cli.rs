// SPDX-License-Identifier: GPL-3.0-or-later
//! Confirmed address-management commands without password arguments.
use crate::broker::{Action, Setup};
use std::path::PathBuf;
#[derive(clap::Args)]
pub(crate) struct Args {
    #[arg(long)]
    config: PathBuf,
    #[arg(long, default_value = "elzevir")]
    service: String,
    #[arg(long)]
    confirm: bool,
    #[arg(long)]
    wait: bool,
    #[command(subcommand)]
    action: Command,
}
#[derive(clap::Subcommand)]
enum Command {
    Enroll {
        #[arg(long)]
        broker: String,
        #[arg(long)]
        institution: String,
        #[arg(long)]
        email: String,
        #[arg(long, default_value = "")]
        name: String,
    },
    Refresh,
    Listing {
        #[arg(long)]
        public: bool,
    },
    Staff {
        #[arg(long)]
        username: String,
        #[arg(long)]
        cidr: Vec<String>,
    },
    DisableStaff,
    Renew,
    Withdraw,
    Export {
        #[arg(long)]
        output: PathBuf,
    },
    Recover {
        #[arg(long)]
        input: PathBuf,
    },
}
pub(crate) fn run(args: Args) -> Result<(), String> {
    if !args.confirm && !matches!(&args.action, Command::Refresh) {
        return Err("address management requires --confirm".to_owned());
    }
    let config = args
        .config
        .canonicalize()
        .map_err(|_| "server configuration unavailable")?;
    let settings = crate::server_config::ServeArgs::for_status(Some(config.clone()), None)
        .resolve()
        .map_err(|error| error.to_string())?;
    let action = match args.action {
        Command::Enroll {
            broker,
            institution,
            email,
            name,
        } => Action::Enroll {
            base: broker,
            institution,
            email,
            name,
        },
        Command::Refresh => Action::Refresh,
        Command::Listing { public } => Action::Listing { public },
        Command::Staff { username, cidr } => {
            let password = rpassword::prompt_password("Remote staff password: ")
                .map_err(|_| "cannot read remote password")?;
            let repeated = zeroize::Zeroizing::new(
                rpassword::prompt_password("Repeat password: ")
                    .map_err(|_| "cannot read password confirmation")?,
            );
            if password != *repeated {
                return Err("password confirmation differs".to_owned());
            }
            Action::Staff {
                username,
                password,
                cidrs: cidr,
            }
        }
        Command::DisableStaff => Action::DisableStaff,
        Command::Renew => Action::Renew,
        Command::Withdraw => Action::Withdraw,
        Command::Export { output } => Action::Export { output },
        Command::Recover { input } => Action::Recover { input },
    };
    let id = crate::host_jobs::launch(
        &settings.data_dir,
        crate::host_jobs::Task::Broker {
            setup: Setup {
                config,
                service: args.service,
                action,
            },
        },
    )?;
    println!("{}", serde_json::json!({"id":id,"state":"queued"}));
    if args.wait {
        crate::host_jobs::wait(&settings.data_dir, &id)?;
    }
    Ok(())
}
