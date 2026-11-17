// SPDX-License-Identifier: GPL-3.0-or-later
//! The same operational jobs and policy as the native panel.

use std::path::PathBuf;

#[derive(clap::Args)]
pub(crate) struct Args {
    #[arg(long, required_unless_present = "data_dir")]
    config: Option<PathBuf>,
    #[arg(long)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    action: Action,
}
#[derive(clap::Subcommand)]
enum Action {
    /// List recent job states and the current policy.
    List,
    /// Verify an existing managed artifact as a background job.
    Verify {
        #[arg(long)]
        id: String,
    },
    /// Set local retention and an optional daily backup schedule.
    Policy {
        #[arg(long)]
        keep: Option<u16>,
        #[arg(long, requires = "minute", conflicts_with = "disable_daily")]
        hour: Option<u8>,
        #[arg(long, requires = "hour")]
        minute: Option<u8>,
        #[arg(long, default_value = "UTC")]
        time_zone: String,
        #[arg(long, conflicts_with = "hour")]
        disable_daily: bool,
    },
}

pub(crate) fn run(args: Args) -> Result<(), String> {
    let settings = crate::server_config::ServeArgs::for_status(args.config, args.data_dir)
        .resolve()
        .map_err(|error| error.to_string())?;
    let info = crate::runtime_info::read(&settings.data_dir.join(crate::runtime_info::NAME))?;
    let output = match args.action {
        Action::List => serde_json::to_value(crate::admin::operations(&info)?),
        Action::Verify { id } => serde_json::to_value(crate::admin::verify_backup(&info, id)?),
        Action::Policy {
            keep,
            hour,
            minute,
            time_zone,
            disable_daily,
        } => {
            let mut policy = crate::admin::operations(&info)?.policy;
            if let Some(keep) = keep {
                policy.keep = keep;
            }
            if let (Some(hour), Some(minute)) = (hour, minute) {
                policy.daily = Some(crate::operations_config::Daily {
                    hour,
                    minute,
                    time_zone,
                });
            }
            if disable_daily {
                policy.daily = None;
            }
            serde_json::to_value(crate::admin::backup_policy(&info, policy)?)
        }
    }
    .map_err(|_| "cannot encode operational response")?;
    println!("{output}");
    Ok(())
}
