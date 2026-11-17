// SPDX-License-Identifier: GPL-3.0-or-later
//! Explicit installation and inspection of optional official tools.
use crate::managed_tools::Tool;
use std::path::PathBuf;
#[derive(clap::Args)]
pub(crate) struct Args {
    #[command(subcommand)]
    action: Action,
}
#[derive(clap::Subcommand)]
enum Action {
    /// Verify the managed binary and print its registered version.
    Status { tool: Tool },
    /// Install or update one official tool as a background client operation.
    Install {
        tool: Tool,
        #[arg(long)]
        data_dir: PathBuf,
        #[arg(long)]
        wait: bool,
    },
}
pub(crate) fn run(args: Args) -> Result<(), String> {
    match args.action {
        Action::Status { tool } => {
            let binary = crate::managed_tools::binary(tool)?;
            let installed = crate::managed_tools::installed(tool)?;
            println!(
                "{}",
                serde_json::json!({"tool":tool.name(),"binary":binary,"version":installed.version})
            );
        }
        Action::Install {
            tool,
            data_dir,
            wait,
        } => {
            let id =
                crate::host_jobs::launch(&data_dir, crate::host_jobs::Task::InstallTool { tool })?;
            println!("{}", serde_json::json!({"id":id,"state":"queued"}));
            if wait {
                crate::host_jobs::wait(&data_dir, &id)?;
            }
        }
    }
    Ok(())
}
