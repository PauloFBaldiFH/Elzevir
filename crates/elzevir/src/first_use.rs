// SPDX-License-Identifier: GPL-3.0-or-later
//! User installation paths, locale and first-use orchestration.

use std::io::{self, IsTerminal};
use std::path::PathBuf;

use crate::server_config::{ServeArgs, Settings};

pub(crate) const CONFIG: &str = "server.toml";

pub(crate) fn data_dir() -> Result<PathBuf, String> {
    let absolute = |key| {
        std::env::var_os(key)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    if cfg!(windows) {
        return absolute("LOCALAPPDATA")
            .map(|path| path.join("Elzevir").join("data"))
            .ok_or_else(|| "LOCALAPPDATA unavailable; pass --data-dir".into());
    }
    absolute("XDG_DATA_HOME")
        .or_else(|| absolute("HOME").map(|home| home.join(".local/share")))
        .map(|path| path.join("elzevir"))
        .ok_or_else(|| "user data directory unavailable; pass --data-dir".into())
}

pub(crate) fn language(explicit: Option<&str>) -> &str {
    explicit.unwrap_or_else(|| {
        let locale = std::env::var("LC_ALL")
            .ok()
            .filter(|value| !value.is_empty())
            .or_else(sys_locale::get_locale);
        if locale.as_deref().is_some_and(|value| {
            value
                .split(['-', '_', '.', '@'])
                .next()
                .is_some_and(|code| code.eq_ignore_ascii_case("pt"))
        }) {
            "pt-BR"
        } else {
            "en"
        }
    })
}

pub(crate) fn interactive() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

pub(crate) fn resolve(mut args: ServeArgs) -> Result<(Settings, Option<PathBuf>), String> {
    if args.config.is_some() || args.data_dir.is_some() {
        let config = args.config.clone();
        return args
            .resolve()
            .map(|settings| (settings, config))
            .map_err(|e| e.to_string());
    }
    let directory = data_dir()?;
    let config = directory.join(CONFIG);
    match std::fs::symlink_metadata(directory.join("elzevir.db")) {
        Ok(meta) if meta.is_file() => {
            // `serve --data-dir` in 0.1.0-alpha.1 created a database with no
            // administrator and no server.toml; point that user to recovery.
            if !config.exists() {
                let messages = elzevir_panel::Messages::new(language(args.lang.as_deref()))
                    .map_err(|error| error.to_string())?;
                eprintln!(
                    "elzevir: {}",
                    messages.values(
                        "setup-no-admin",
                        &[("dir", directory.display().to_string())]
                    )
                );
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if !interactive() {
                return Err("run elzevir init first, or use an interactive terminal (panel --demo is also available)".into());
            }
            let messages = elzevir_panel::Messages::new(language(args.lang.as_deref()))
                .map_err(|error| error.to_string())?;
            if !crate::init::confirm_setup(&messages)? {
                return Err(messages.get("setup-cancelled"));
            }
            let mut init = crate::init::InitArgs::first_use(
                args.lang.clone(),
                args.listen,
                args.insecure_cookies,
            );
            // Nothing is written yet, so the chosen port goes into the new server.toml.
            init.listen =
                crate::listen_port::ensure(init.listen, None, language(args.lang.as_deref()))?;
            crate::init::run(&init)?;
            println!(
                "elzevir: {}",
                messages.values(
                    "setup-done",
                    &[
                        ("listen", init.listen.to_string()),
                        ("login", init.login.clone()),
                    ]
                )
            );
        }
        _ => return Err("cannot inspect default institution database".into()),
    }
    match std::fs::symlink_metadata(&config) {
        Ok(meta) if meta.is_file() => args.config = Some(config),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        _ => return Err("cannot inspect default institution configuration".into()),
    }
    args.data_dir = Some(directory);
    let config = args.config.clone();
    args.resolve()
        .map(|settings| (settings, config))
        .map_err(|e| e.to_string())
}

pub(crate) fn start(args: ServeArgs, no_self_update: bool) -> Result<(), String> {
    if !interactive() {
        return Err(
            "start requires an interactive terminal; use serve for unattended startup".into(),
        );
    }
    // A --listen override is never written back to server.toml.
    let explicit_listen = args.listen.is_some();
    let (settings, config) = resolve(args)?;
    if matches!(
        crate::status::inspect(&settings.data_dir).state,
        "running" | "degraded"
    ) {
        return crate::panel_cli::connected(&settings, config, false, no_self_update, || false);
    }
    let mut settings = settings;
    let saved = config.as_deref().filter(|_| !explicit_listen);
    settings.listen = crate::listen_port::ensure(settings.listen, saved, &settings.lang)?;
    let control = crate::shutdown::Control::default();
    std::thread::scope(|scope| {
        let (ready, started) = std::sync::mpsc::sync_channel(1);
        let server_control = control.clone();
        let settings_ref = &settings;
        let server = std::thread::Builder::new()
            .name("elzevir-web".into())
            .spawn_scoped(scope, move || {
                let result = crate::serve_controlled(settings_ref, &server_control, || {
                    ready.send(()).map_err(|_| "start panel unavailable".into())
                });
                server_control.request();
                result
            })
            .map_err(|error| error.to_string())?;
        let _stop = StopOnDrop(&control);
        let panel = if started.recv().is_ok() {
            let stopping = control.clone();
            crate::panel_cli::connected(&settings, config, false, no_self_update, move || {
                stopping.requested()
            })
        } else {
            Err("server stopped before the panel could open".into())
        };
        control.request();
        server
            .join()
            .map_err(|_| "integrated server thread failed")??;
        panel
    })
}

struct StopOnDrop<'a>(&'a crate::shutdown::Control);

impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.request();
    }
}
