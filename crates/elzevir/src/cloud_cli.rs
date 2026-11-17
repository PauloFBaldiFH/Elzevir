// SPDX-License-Identifier: GPL-3.0-or-later
//! Cloud intentions run under the limited service account, never the elevated client.
use std::{
    io::{BufRead as _, Read as _},
    path::PathBuf,
};
#[derive(clap::Args)]
pub(crate) struct Args {
    #[arg(long)]
    config: PathBuf,
    #[command(subcommand)]
    action: Command,
}
#[derive(clap::Subcommand)]
enum Command {
    Plan {
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        name: String,
        #[arg(long)]
        connection: String,
        #[arg(long, default_value = "Elzevir/backups")]
        folder: String,
        #[arg(long, default_value_t = 7)]
        keep: u16,
        #[arg(long, default_value = "")]
        time: String,
        #[arg(long, default_value = "UTC")]
        zone: String,
    },
    RunPlan {
        #[arg(long)]
        id: String,
    },
    DisablePlan {
        #[arg(long)]
        id: String,
    },
    RemovePlan {
        #[arg(long)]
        id: String,
    },
    RemoveConnection {
        #[arg(long)]
        id: String,
    },
    Status,
    Providers,
    Connect {
        #[arg(long)]
        name: String,
        #[arg(long)]
        provider: String,
    },
    Crypt {
        #[arg(long)]
        name: String,
        #[arg(long)]
        parent: String,
        #[arg(long, default_value = "Elzevir")]
        folder: String,
    },
    Answer {
        #[arg(long)]
        id: String,
        #[arg(long)]
        answer_file: Option<PathBuf>,
    },
    Test {
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "Elzevir/tests")]
        folder: String,
    },
}
pub(crate) fn run(args: Args) -> Result<(), String> {
    let settings = crate::server_config::ServeArgs::for_status(Some(args.config), None)
        .resolve()
        .map_err(|error| error.to_string())?;
    let directory = settings.data_dir;
    let action = match args.action {
        Command::Plan {
            id,
            name,
            connection,
            folder,
            keep,
            time,
            zone,
        } => crate::cloud::Action::Plan {
            definition: crate::cloud_plans::Definition {
                id,
                name,
                connection,
                folder,
                keep,
                daily: crate::cloud::daily(&time, &zone)?,
            },
        },
        Command::RunPlan { id } => crate::cloud::Action::RunPlan { id },
        Command::DisablePlan { id } => crate::cloud::Action::DisablePlan { id },
        Command::RemovePlan { id } => crate::cloud::Action::RemovePlan { id },
        Command::RemoveConnection { id } => crate::cloud::Action::RemoveConnection { id },
        Command::Status => {
            println!(
                "{}",
                serde_json::to_string(&crate::cloud::view(&directory)?)
                    .map_err(|_| "cannot encode cloud status")?
            );
            return Ok(());
        }
        Command::Providers => crate::cloud::Action::Providers,
        Command::Connect { name, provider } => crate::cloud::Action::Begin {
            name,
            provider,
            parent: None,
        },
        Command::Crypt {
            name,
            parent,
            folder,
        } => crate::cloud::Action::Begin {
            name,
            provider: "crypt".to_owned(),
            parent: Some(crate::cloud::Parent { id: parent, folder }),
        },
        Command::Test { id, folder } => crate::cloud::Action::Test { id, folder },
        Command::Answer { id, answer_file } => {
            let question = crate::cloud::view(&directory)?
                .question
                .filter(|question| question.id == id)
                .ok_or("no pending provider question for this connection")?;
            let mut value = zeroize::Zeroizing::new(String::new());
            if let Some(path) = answer_file {
                if crate::backup_archive::regular(&path)? > 16_384 {
                    return Err("cloud answer file exceeds limit".to_owned());
                }
                std::fs::File::open(path)
                    .and_then(|file| file.take(16_385).read_to_string(&mut value))
                    .map_err(|_| "private cloud answer file unavailable")?;
            } else if question.secret {
                *value = rpassword::prompt_password("Private provider answer: ")
                    .map_err(|_| "private input unavailable")?;
            } else {
                std::io::stdin()
                    .lock()
                    .take(16_385)
                    .read_line(&mut value)
                    .map_err(|_| "provider input unavailable")?;
            }
            if value.len() > 16_384 {
                return Err("cloud answer exceeds limit".to_owned());
            }
            crate::cloud::Action::Answer {
                id,
                revision: question.revision,
                value: value.trim_end_matches(['\n', '\r']).to_owned(),
            }
        }
    };
    let info = crate::runtime_info::read(&directory.join(crate::runtime_info::NAME))?;
    let job = crate::admin::cloud(&info, action)?;
    println!(
        "{}",
        serde_json::to_string(&job).map_err(|_| "cannot encode cloud job")?
    );
    Ok(())
}
