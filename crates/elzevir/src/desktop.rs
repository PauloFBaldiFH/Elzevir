// SPDX-License-Identifier: GPL-3.0-or-later
//! Desktop launcher for the panel, so daily use needs no typed command.
//!
//! Linux follows the freedesktop.org specifications: an application entry
//! with `Terminal=true` lets the desktop open its own terminal, whichever it
//! is. Windows shortcuts are created by `install.ps1`.

use std::path::{Path, PathBuf};

const ENTRY: &str = "elzevir.desktop";
const ICON: &str = include_str!("../../../docs/marca/elzevir-logo.svg");

/// Arguments of `elzevir desktop`.
#[derive(clap::Args)]
pub(crate) struct Args {
    #[command(subcommand)]
    action: Action,
}

#[derive(clap::Subcommand)]
enum Action {
    /// Add the Elzevir launcher to the application menu and the desktop.
    Install,
    /// Remove the launcher and its icon.
    Remove,
}

/// Where the launcher files live for the current user.
struct Places {
    entry: PathBuf,
    icon: PathBuf,
    desktop: Option<PathBuf>,
}

pub(crate) fn run(args: &Args) -> Result<(), String> {
    if !cfg!(target_os = "linux") {
        return Err("desktop launchers are created by the installer on this system".into());
    }
    let home = absolute_env("HOME").ok_or("HOME unavailable")?;
    let data = absolute_env("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share"));
    let config = absolute_env("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
    let user_dirs = std::fs::read_to_string(config.join("user-dirs.dirs")).unwrap_or_default();
    let places = Places {
        entry: data.join("applications").join(ENTRY),
        icon: data.join("icons/hicolor/scalable/apps/elzevir.svg"),
        desktop: Some(desktop_dir(&user_dirs, &home)).filter(|path| path.is_dir()),
    };
    match args.action {
        Action::Install => {
            let executable =
                std::env::current_exe().map_err(|_| "cannot locate the elzevir executable")?;
            install(&places, &entry(&executable)?)
        }
        Action::Remove => remove(&places),
    }
}

fn absolute_env(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// The desktop folder from `user-dirs.dirs`, which localized systems rename
/// (for example "Área de Trabalho"); `~/Desktop` otherwise.
fn desktop_dir(user_dirs: &str, home: &Path) -> PathBuf {
    user_dirs
        .lines()
        .filter_map(|line| line.trim().strip_prefix("XDG_DESKTOP_DIR="))
        .filter_map(|value| value.strip_prefix('"')?.strip_suffix('"'))
        .find_map(|value| match value.strip_prefix("$HOME") {
            Some(rest) => Some(home.join(rest.trim_start_matches('/'))),
            None => Some(PathBuf::from(value)).filter(|path| path.is_absolute()),
        })
        .unwrap_or_else(|| home.join("Desktop"))
}

/// Quotes one Exec argument as the Desktop Entry Specification requires.
fn exec_argument(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for character in value.chars() {
        match character {
            '"' | '`' | '$' | '\\' => {
                quoted.push_str("\\\\");
                quoted.push(character);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

fn entry(executable: &Path) -> Result<String, String> {
    let path = executable
        .to_str()
        .filter(|path| !path.chars().any(char::is_control))
        .ok_or("the executable path cannot be written in a launcher")?;
    let english = elzevir_panel::Messages::new("en").map_err(|error| error.to_string())?;
    let portuguese = elzevir_panel::Messages::new("pt-BR").map_err(|error| error.to_string())?;
    Ok(format!(
        "[Desktop Entry]\nType=Application\nVersion=1.5\nName=Elzevir\n\
         Comment={}\nComment[pt_BR]={}\nExec={} start\nIcon=elzevir\n\
         Terminal=true\nCategories=Office;Education;\nStartupNotify=false\n",
        english.get("desktop-comment"),
        portuguese.get("desktop-comment"),
        exec_argument(path),
    ))
}

fn install(places: &Places, entry: &str) -> Result<(), String> {
    write(&places.icon, ICON, 0o644)?;
    write(&places.entry, entry, 0o644)?;
    if let Some(desktop) = &places.desktop {
        let launcher = desktop.join(ENTRY);
        // Desktops only run launchers on the desktop folder when executable.
        write(&launcher, entry, 0o755)?;
        trust(&launcher);
    }
    Ok(())
}

fn write(path: &Path, text: &str, mode: u32) -> Result<(), String> {
    let parent = path.parent().ok_or("launcher folder unavailable")?;
    std::fs::create_dir_all(parent).map_err(|_| "cannot create launcher folder")?;
    let mut file =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| "cannot stage launcher file")?;
    std::io::Write::write_all(&mut file, text.as_bytes())
        .map_err(|_| "cannot write launcher file")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(mode))
            .map_err(|_| "cannot set launcher permissions")?;
    }
    #[cfg(not(unix))]
    let _ = mode; // Permission bits only exist on Unix.
    file.persist(path)
        .map_err(|_| "cannot publish launcher file")?;
    Ok(())
}

/// GNOME asks before running an untrusted desktop launcher; mark ours trusted.
fn trust(launcher: &Path) {
    // Optional: without `gio` (or outside GNOME) the launcher still works
    // after the desktop's own "Allow launching" confirmation.
    drop(
        std::process::Command::new("gio")
            .args(["set", "--type=string"])
            .arg(launcher)
            .args(["metadata::trusted", "true"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status(),
    );
}

fn remove(places: &Places) -> Result<(), String> {
    let desktop = places.desktop.as_ref().map(|folder| folder.join(ENTRY));
    for path in [Some(&places.entry), Some(&places.icon), desktop.as_ref()]
        .into_iter()
        .flatten()
    {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(format!("cannot remove {}", path.display())),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localized_desktop_folders_are_followed() {
        let home = Path::new("/home/reader");
        let dirs = "# comment\nXDG_DESKTOP_DIR=\"$HOME/Área de Trabalho\"\n";
        assert_eq!(
            desktop_dir(dirs, home),
            PathBuf::from("/home/reader/Área de Trabalho")
        );
        // An absolute path in this platform's own form.
        let absolute = std::env::temp_dir().join("desk");
        let line = format!("XDG_DESKTOP_DIR=\"{}\"", absolute.display());
        assert_eq!(desktop_dir(&line, home), absolute);
        assert_eq!(desktop_dir("", home), PathBuf::from("/home/reader/Desktop"));
        assert_eq!(
            desktop_dir("XDG_DESKTOP_DIR=\"relative\"", home),
            PathBuf::from("/home/reader/Desktop")
        );
    }

    #[test]
    fn exec_paths_are_quoted_for_the_desktop_specification() {
        assert_eq!(
            exec_argument("/home/a b/.local/bin/elzevir"),
            "\"/home/a b/.local/bin/elzevir\""
        );
        assert_eq!(exec_argument("/x/$y%z"), "\"/x/\\\\$y%%z\"");
    }

    #[test]
    fn the_entry_opens_the_panel_in_a_terminal() {
        let text = entry(Path::new("/opt/elzevir/elzevir")).unwrap();
        assert!(text.contains("Exec=\"/opt/elzevir/elzevir\" start\n"));
        assert!(text.contains("Terminal=true\n"));
        assert!(text.contains("Comment[pt_BR]=Abrir o painel do Elzevir\n"));
        assert!(entry(Path::new("/bad\npath")).is_err());
    }

    #[test]
    fn install_and_remove_manage_only_the_launcher_files() {
        let root = tempfile::tempdir().unwrap();
        let desktop = root.path().join("Desktop");
        std::fs::create_dir(&desktop).unwrap();
        let places = Places {
            entry: root.path().join("share/applications/elzevir.desktop"),
            icon: root.path().join("share/icons/elzevir.svg"),
            desktop: Some(desktop.clone()),
        };
        install(&places, "[Desktop Entry]\n").unwrap();
        assert!(places.entry.is_file() && places.icon.is_file());
        let launcher = desktop.join(ENTRY);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&launcher).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755);
        }
        remove(&places).unwrap();
        assert!(!places.entry.exists() && !launcher.exists() && desktop.is_dir());
        remove(&places).unwrap();
    }
}
