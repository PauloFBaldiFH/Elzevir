// SPDX-License-Identifier: GPL-3.0-or-later
//! Native tunnel intentions share the panel's confirmed client job pipeline.
use std::path::PathBuf;
#[derive(clap::Args)]
pub(crate) struct Args {
    #[arg(long, required_unless_present = "data_dir")]
    config: Option<PathBuf>,
    #[arg(long)]
    data_dir: Option<PathBuf>,
    #[arg(long)]
    wait: bool,
    #[command(subcommand)]
    action: Action,
}
#[derive(clap::Subcommand)]
enum Action {
    /// Sign in to the operator's Cloudflare account; reuses an existing private certificate.
    Login,
    /// Test public HTTPS endpoints as a bounded background job.
    Test,
    /// Print safe observed public candidates and redacted log events.
    Status,
    /// Publish own-domain hosts with the staff interface isolated by hostname.
    Publish {
        #[arg(long)]
        catalog: String,
        #[arg(long)]
        staff: String,
        #[arg(long, default_value = "")]
        name: String,
        #[arg(long, default_value = "elzevir")]
        service: String,
        #[arg(long)]
        http2: bool,
        #[arg(long)]
        confirm_publish: bool,
    },
}
pub(crate) fn run(args: Args) -> Result<(), String> {
    let settings = crate::server_config::ServeArgs::for_status(args.config.clone(), args.data_dir)
        .resolve()
        .map_err(|error| error.to_string())?;
    let task = match args.action {
        Action::Login => crate::host_jobs::Task::CloudflareLogin,
        Action::Test => crate::host_jobs::Task::TestTunnel,
        Action::Status => {
            let observed = crate::tunnel_observe::status(&settings.data_dir)?;
            println!("{observed}");
            return Ok(());
        }
        Action::Publish {
            catalog,
            staff,
            name,
            service,
            http2,
            confirm_publish,
        } => {
            if !confirm_publish {
                return Err("publishing requires --confirm-publish".to_owned());
            }
            crate::host_jobs::Task::CloudflareSetup {
                setup: crate::cloudflare::Setup {
                    config: args
                        .config
                        .ok_or("publishing requires a server configuration file")?
                        .canonicalize()
                        .map_err(|_| "server configuration unavailable")?,
                    catalog,
                    staff,
                    name,
                    service,
                    protocol: if http2 { "http2" } else { "auto" }.to_owned(),
                },
            }
        }
    };
    let id = crate::host_jobs::launch(&settings.data_dir, task)?;
    println!(
        "{}",
        serde_json::json!({"id":id,"state":"queued","follow":"open panel Addresses to see the authorization link"})
    );
    if args.wait {
        crate::host_jobs::wait(&settings.data_dir, &id)?;
    }
    Ok(())
}
