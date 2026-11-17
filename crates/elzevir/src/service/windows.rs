// SPDX-License-Identifier: GPL-3.0-or-later
//! Native Windows SCM registration, control and process entry.

use super::{Backend, Entry, Plan};
use std::{
    ffi::OsString,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows_service::{
    service::{
        Service, ServiceAccess, ServiceAction, ServiceActionType, ServiceControl,
        ServiceControlAccept, ServiceErrorControl, ServiceExitCode, ServiceFailureActions,
        ServiceFailureResetPeriod, ServiceInfo, ServiceSidType, ServiceStartType, ServiceState,
        ServiceStatus, ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult, ServiceStatusHandle},
    service_manager::{ServiceManager, ServiceManagerAccess},
};

pub(super) struct Windows;
fn marker(name: &str) -> String {
    format!("Elzevir managed v1: {name}")
}
fn error(_: windows_service::Error) -> String {
    "Windows service operation failed; check elevation, account permissions and Event Viewer"
        .to_owned()
}

fn manager(create: bool) -> Result<ServiceManager, String> {
    let rights = if create {
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE
    } else {
        ServiceManagerAccess::CONNECT
    };
    ServiceManager::local_computer(None::<&str>, rights).map_err(error)
}

fn owned(manager: &ServiceManager, name: &str, rights: ServiceAccess) -> Result<Service, String> {
    let service = manager
        .open_service(name, rights | ServiceAccess::QUERY_CONFIG)
        .map_err(error)?;
    let config = service.query_config().map_err(error)?;
    let account = format!("NT SERVICE\\{name}");
    if config.display_name != OsString::from(marker(name))
        || config.service_type != ServiceType::OWN_PROCESS
        || config
            .account_name
            .as_ref()
            .is_none_or(|a| !a.to_string_lossy().eq_ignore_ascii_case(&account))
    {
        return Err("service is not managed by Elzevir; it was preserved".to_owned());
    }
    Ok(service)
}

fn wait_for(service: &Service, state: ServiceState) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_mins(1);
    loop {
        let current = service.query_status().map_err(error)?;
        if current.current_state == state {
            return Ok(());
        }
        if state == ServiceState::Running && current.current_state == ServiceState::Stopped {
            return Err(
                "service stopped before reaching Running; inspect permissions and Event Viewer"
                    .to_owned(),
            );
        }
        if Instant::now() >= deadline {
            return Err("service state transition timed out".to_owned());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn stop(service: &Service) -> Result<(), String> {
    match service.query_status().map_err(error)?.current_state {
        ServiceState::Stopped => return Ok(()),
        ServiceState::StopPending => {}
        _ => {
            service.stop().map_err(error)?;
        }
    }
    wait_for(service, ServiceState::Stopped)
}

impl Backend for Windows {
    fn install(&self, plan: &Plan) -> Result<(), String> {
        let manager = manager(true)?;
        let info = ServiceInfo {
            name: plan.name.clone().into(),
            display_name: marker(&plan.name).into(),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: plan.executable.clone(),
            launch_arguments: vec![
                "service".into(),
                "run".into(),
                "--name".into(),
                plan.name.clone().into(),
                "--config".into(),
                plan.config.as_os_str().to_owned(),
            ],
            dependencies: vec![],
            account_name: Some(plan.account.clone().into()),
            account_password: None,
        };
        // create_service refuses existing services; never open-and-reconfigure as fallback.
        let service = manager
            .create_service(
                &info,
                ServiceAccess::CHANGE_CONFIG | ServiceAccess::DELETE | ServiceAccess::START,
            )
            .map_err(error)?;
        let configured = (|| {
            service.set_description("Managed by Elzevir service v1; configuration and data are preserved on uninstall.").map_err(error)?;
            service
                .set_config_service_sid_info(ServiceSidType::Unrestricted)
                .map_err(error)?;
            service
                .update_failure_actions(ServiceFailureActions {
                    reset_period: ServiceFailureResetPeriod::After(Duration::from_hours(24)),
                    reboot_msg: None,
                    command: None,
                    actions: Some(vec![ServiceAction {
                        action_type: ServiceActionType::Restart,
                        delay: Duration::from_secs(5),
                    }]),
                })
                .map_err(error)?;
            service
                .set_failure_actions_on_non_crash_failures(true)
                .map_err(error)
        })();
        if configured.is_err() && service.delete().is_err() {
            return Err("service installation failed and rollback was incomplete".to_owned());
        }
        configured
    }

    fn action(&self, action: &str, name: &str) -> Result<(), String> {
        let manager = manager(false)?;
        let service = owned(
            &manager,
            name,
            ServiceAccess::QUERY_STATUS
                | ServiceAccess::START
                | ServiceAccess::STOP
                | ServiceAccess::DELETE,
        )?;
        match action {
            "uninstall" => {
                stop(&service)?;
                service.delete().map_err(error)
            }
            "stop" => stop(&service),
            "start" | "restart" => {
                if action == "restart" {
                    stop(&service)?;
                }
                if service.query_status().map_err(error)?.current_state != ServiceState::Running {
                    service.start::<OsString>(&[]).map_err(error)?;
                    wait_for(&service, ServiceState::Running)?;
                }
                Ok(())
            }
            _ => Err("unknown service action".to_owned()),
        }
    }

    fn status(&self, name: &str) -> Result<serde_json::Value, String> {
        let manager = manager(false)?;
        // ERROR_SERVICE_DOES_NOT_EXIST (1060) is an ordinary observation, not success.
        let service = match manager.open_service(
            name,
            ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG,
        ) {
            Err(windows_service::Error::Winapi(e)) if e.raw_os_error() == Some(1060) => {
                return Ok(
                    serde_json::json!({"schema":1,"manager":"windows","name":name,"state":"not_installed"}),
                );
            }
            Err(e) => return Err(error(e)),
            Ok(service) => service,
        };
        drop(service);
        let service = owned(&manager, name, ServiceAccess::QUERY_STATUS)?;
        let status = service.query_status().map_err(error)?;
        let state = match status.current_state {
            ServiceState::Running => "running",
            ServiceState::Stopped => "stopped",
            ServiceState::StartPending => "starting",
            ServiceState::StopPending => "stopping",
            _ => "unavailable",
        };
        Ok(
            serde_json::json!({"schema":1,"manager":"windows","name":name,"state":state,"pid":status.process_id}),
        )
    }
}

struct Context {
    entry: Entry,
    failed: AtomicBool,
}
static CONTEXT: OnceLock<Context> = OnceLock::new();

pub(super) fn dispatch(entry: Entry) -> Result<(), String> {
    let name = entry.name.clone();
    CONTEXT
        .set(Context {
            entry,
            failed: AtomicBool::new(false),
        })
        .map_err(|_| "native service entry already initialized")?;
    windows_service::service_dispatcher::start(&name, ffi_entry).map_err(error)?;
    if CONTEXT
        .get()
        .is_none_or(|c| c.failed.load(Ordering::Acquire))
    {
        Err("native service failed during startup or shutdown".to_owned())
    } else {
        Ok(())
    }
}

windows_service::define_windows_service!(ffi_entry, service_main);

fn service_main(_arguments: Vec<OsString>) {
    if let Some(context) = CONTEXT.get()
        && run_service(&context.entry).is_err()
    {
        context.failed.store(true, Ordering::Release);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Phase {
    Starting,
    Running,
    Stopping,
    Stopped,
}

struct Reporter {
    handle: Option<ServiceStatusHandle>,
    phase: Phase,
    failed: bool,
}
impl Reporter {
    fn report(&mut self, phase: Phase) -> Result<(), String> {
        self.phase = self.phase.max(phase);
        let (current_state, controls_accepted, checkpoint, wait_hint) = match self.phase {
            Phase::Starting => (
                ServiceState::StartPending,
                ServiceControlAccept::empty(),
                1,
                Duration::from_mins(1),
            ),
            Phase::Running => (
                ServiceState::Running,
                ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
                0,
                Duration::ZERO,
            ),
            Phase::Stopping => (
                ServiceState::StopPending,
                ServiceControlAccept::empty(),
                1,
                Duration::from_secs(45),
            ),
            Phase::Stopped => (
                ServiceState::Stopped,
                ServiceControlAccept::empty(),
                0,
                Duration::ZERO,
            ),
        };
        if let Some(handle) = self.handle
            && handle
                .set_service_status(ServiceStatus {
                    service_type: ServiceType::OWN_PROCESS,
                    current_state,
                    controls_accepted,
                    exit_code: ServiceExitCode::Win32(u32::from(self.failed)),
                    checkpoint,
                    wait_hint,
                    process_id: None,
                })
                .is_err()
        {
            self.failed = true;
            return Err("cannot report Windows service state".to_owned());
        }
        Ok(())
    }
}

fn run_service(entry: &Entry) -> Result<(), String> {
    let control = crate::shutdown::Control::native();
    let stopping = control.clone();
    let reporter = Arc::new(Mutex::new(Reporter {
        handle: None,
        phase: Phase::Starting,
        failed: false,
    }));
    let events = Arc::clone(&reporter);
    let handle = service_control_handler::register(&entry.name, move |event| match event {
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        ServiceControl::Stop | ServiceControl::Shutdown => {
            stopping.request();
            if let Ok(mut reporter) = events.lock() {
                // The stop request stands even if the SCM status call fails.
                drop(reporter.report(Phase::Stopping));
            }
            ServiceControlHandlerResult::NoError
        }
        _ => ServiceControlHandlerResult::NotImplemented,
    })
    .map_err(error)?;
    {
        let mut state = reporter.lock().map_err(|_| "service state unavailable")?;
        state.handle = Some(handle);
        state.report(Phase::Starting)?;
    }
    let result = (|| {
        let settings =
            crate::server_config::ServeArgs::for_status(Some(entry.config.clone()), None)
                .resolve()
                .map_err(|e| e.to_string())?;
        crate::serve_controlled(&settings, &control, || {
            reporter
                .lock()
                .map_err(|_| "service state unavailable")?
                .report(Phase::Running)
        })
    })();
    let mut state = reporter.lock().map_err(|_| "service state unavailable")?;
    state.failed |= result.is_err();
    state.report(Phase::Stopped)?;
    if state.failed {
        Err("native service failed".to_owned())
    } else {
        result
    }
}
