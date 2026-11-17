// SPDX-License-Identifier: GPL-3.0-or-later
//! Restricted Windows ACL operations, independently testable from the host runner.
use crate::process_runner::{Request, Runner};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Copy)]
pub(super) enum Scope {
    Operator,
    SharedTools,
}

pub(super) fn protect(
    path: &Path,
    directory: bool,
    scope: Scope,
    program: &Path,
    runner: &impl Runner,
) -> Result<(), String> {
    if matches!(scope, Scope::SharedTools) {
        apply(
            program,
            runner,
            vec![path.as_os_str().to_owned(), "/reset".into()],
            "cannot reset shared tool directory ACL",
        )?;
    }
    let rights = if directory { "(OI)(CI)F" } else { "F" };
    let mut args = vec![
        path.as_os_str().to_owned(),
        "/inheritance:r".into(),
        "/grant:r".into(),
        format!("*S-1-5-18:{rights}").into(),
        format!("*S-1-5-32-544:{rights}").into(),
    ];
    if matches!(scope, Scope::SharedTools) {
        args.push("*S-1-5-6:(OI)(CI)RX".into());
    }
    // icacls uses separate command forms for DACL changes and changing the owner.
    apply(program, runner, args, "cannot set restricted Windows DACL")?;
    apply(
        program,
        runner,
        vec![
            path.as_os_str().to_owned(),
            "/setowner".into(),
            "*S-1-5-32-544".into(),
        ],
        "cannot set administrator ownership",
    )
}
fn apply(
    program: &Path,
    runner: &impl Runner,
    args: Vec<OsString>,
    failure: &str,
) -> Result<(), String> {
    let output = runner.run(Request {
        environment: None,
        progress: None,
        program: PathBuf::from(program),
        args,
        input: None,
        timeout: Duration::from_secs(10),
        limit: 4096,
    })?;
    if !output.success {
        return Err(failure.to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_runner::Output;
    use std::cell::RefCell;
    #[derive(Default)]
    struct Icacls {
        calls: RefCell<Vec<Vec<OsString>>>,
        fail: Option<usize>,
    }
    impl Runner for Icacls {
        fn run(&self, request: Request) -> Result<Output, String> {
            let owns = request.args.iter().any(|arg| arg == "/setowner");
            let valid = !owns || request.args.len() == 3;
            self.calls.borrow_mut().push(request.args);
            let success = valid && self.fail != Some(self.calls.borrow().len());
            Ok(Output {
                success,
                stdout: vec![],
            })
        }
    }
    #[test]
    fn operator_and_shared_tool_acls_use_valid_commands_and_keep_their_distinct_grants() {
        for (scope, shared) in [(Scope::Operator, false), (Scope::SharedTools, true)] {
            for directory in [false, true] {
                let runner = Icacls::default();
                protect(
                    Path::new("private path"),
                    directory,
                    scope,
                    Path::new("icacls.exe"),
                    &runner,
                )
                .unwrap();
                let calls = runner.calls.borrow();
                let grants = calls
                    .iter()
                    .find(|args| args.iter().any(|arg| arg == "/grant:r"))
                    .unwrap();
                assert!(grants.iter().any(|arg| arg == "/inheritance:r"));
                assert_eq!(
                    grants.iter().any(|arg| arg == "*S-1-5-6:(OI)(CI)RX"),
                    shared
                );
                assert!(calls.last().unwrap().iter().any(|arg| arg == "/setowner"));
                assert_eq!(calls.len(), if shared { 3 } else { 2 });
            }
        }
    }
    #[test]
    fn acl_failure_stops_before_any_later_mutation() {
        for failed in 1..=3 {
            let runner = Icacls {
                fail: Some(failed),
                ..Icacls::default()
            };
            assert!(
                protect(
                    Path::new("private path"),
                    true,
                    Scope::SharedTools,
                    Path::new("icacls.exe"),
                    &runner
                )
                .is_err()
            );
            assert_eq!(runner.calls.borrow().len(), failed);
        }
    }
}

#[cfg(all(test, windows))]
mod native_tests {
    use super::*;
    use serde::Deserialize;
    use std::process::Command;
    #[derive(Deserialize)]
    struct Security {
        owner: String,
        protected: bool,
        rules: Vec<Rule>,
    }
    #[derive(Deserialize)]
    struct Rule {
        sid: String,
        rights: u32,
        inherited: bool,
        kind: String,
    }
    fn security(path: &Path) -> Security {
        let script = r#"
$ErrorActionPreference = 'Stop'
$acl = Get-Acl -LiteralPath $env:ELZEVIR_ACL_TEST_PATH
$sid = [System.Security.Principal.SecurityIdentifier]
$rules = @($acl.GetAccessRules($true,$true,$sid) | ForEach-Object {
    @{ sid = $_.IdentityReference.Value; rights = [int]$_.FileSystemRights; inherited = $_.IsInherited; kind = $_.AccessControlType.ToString() }
})
@{ owner = $acl.GetOwner($sid).Value; protected = $acl.AreAccessRulesProtected; rules = $rules } | ConvertTo-Json -Depth 4 -Compress
"#;
        let powershell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let output = Command::new(powershell)
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                script,
            ])
            .env("ELZEVIR_ACL_TEST_PATH", path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    #[test]
    fn native_operator_files_and_shared_tools_retain_the_required_owner_and_access() {
        let root = tempfile::tempdir().unwrap();
        for (name, directory, shared) in [
            ("operator directory", true, false),
            ("operator file", false, false),
            ("shared tools", true, true),
        ] {
            let path = root.path().join(name);
            if directory {
                std::fs::create_dir(&path).unwrap();
            } else {
                std::fs::write(&path, "fixture").unwrap();
            }
            // Recovery supplies canonical Windows paths, including the verbatim prefix.
            let canonical = path.canonicalize().unwrap();
            if shared {
                crate::file_owner::program_directory(&canonical).unwrap();
            } else {
                crate::file_owner::operator_private(&canonical).unwrap();
            }
            let acl = security(&path);
            assert_eq!(acl.owner, "S-1-5-32-544");
            assert!(acl.protected);
            for rule in &acl.rules {
                assert_eq!(rule.kind, "Allow");
                assert!(!rule.inherited);
                match rule.sid.as_str() {
                    "S-1-5-18" | "S-1-5-32-544" => assert_eq!(rule.rights & 0x1f_01ff, 0x1f_01ff),
                    "S-1-5-6" if shared => {
                        assert_eq!(rule.rights & 0x12_00a9, 0x12_00a9);
                        assert_eq!(rule.rights & (0x2 | 0x4 | 0x100 | 0x4_0000 | 0x8_0000), 0);
                    }
                    _ => panic!("unexpected ACL principal: {}", rule.sid),
                }
            }
            assert!(acl.rules.iter().any(|rule| rule.sid == "S-1-5-18"));
            assert!(acl.rules.iter().any(|rule| rule.sid == "S-1-5-32-544"));
            assert_eq!(acl.rules.iter().any(|rule| rule.sid == "S-1-5-6"), shared);
        }
    }
}
