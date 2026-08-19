use crate::config::{
    default_data_dir, install_dir, AppConfig, DEFAULT_LISTEN, DISPLAY_NAME, SERVICE_NAME,
};
use anyhow::{bail, Context, Result};
use std::path::PathBuf;

#[cfg(windows)]
#[allow(unused_must_use)]
mod windows_impl {
    use super::*;
    use crate::{app::AppState, web};
    use std::ffi::OsString;
    use std::fs;
    use std::sync::mpsc;
    use std::time::Duration;
    use windows_service::{
        define_windows_service,
        service::{
            ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceInfo,
            ServiceStartType, ServiceState, ServiceStatus, ServiceType,
        },
        service_control_handler::{self, ServiceControlHandlerResult},
        service_dispatcher,
        service_manager::{ServiceManager, ServiceManagerAccess},
    };

    define_windows_service!(ffi_service_main, service_main);

    pub fn run_service() -> Result<()> {
        service_dispatcher::start(SERVICE_NAME, ffi_service_main)
            .context("start Windows service dispatcher")
    }

    fn service_main(_arguments: Vec<OsString>) -> std::result::Result<(), windows_service::Error> {
        let (stop_sender, stop_receiver) = mpsc::channel::<()>();
        let status_handle =
            service_control_handler::register(SERVICE_NAME, move |event| match event {
                ServiceControl::Stop | ServiceControl::Shutdown => {
                    let _ = stop_sender.send(());
                    ServiceControlHandlerResult::NoError
                }
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                _ => ServiceControlHandlerResult::NotImplemented,
            })?;

        status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Running,
            controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
            exit_code: windows_service::service::ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })?;

        let config = AppConfig::new(DEFAULT_LISTEN, Some(default_data_dir()), true)
            .map_err(|_| service_error())?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|_| service_error())?;
        let result = runtime.block_on(async move {
            let state = AppState::initialize(config).await.map_err(|_| ())?;
            let shutdown = async move {
                let _ = tokio::task::spawn_blocking(move || stop_receiver.recv()).await;
            };
            web::serve_foreground(state, shutdown).await.map_err(|_| ())
        });

        let _ = status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: windows_service::service::ServiceExitCode::Win32(if result.is_ok() {
                0
            } else {
                1
            }),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        });
        result.map_err(|_| service_error())
    }

    pub fn install() -> Result<()> {
        let current_exe = std::env::current_exe().context("resolve current executable")?;
        let service_dir = install_dir();
        let data_dir = default_data_dir();
        fs::create_dir_all(&service_dir)
            .with_context(|| format!("create {}", service_dir.display()))?;
        fs::create_dir_all(&data_dir).with_context(|| format!("create {}", data_dir.display()))?;
        let target_exe = service_dir.join("wakebridge.exe");
        if current_exe != target_exe {
            fs::copy(&current_exe, &target_exe).with_context(|| {
                format!("copy {} to {}", current_exe.display(), target_exe.display())
            })?;
        }
        grant_data_directory(&data_dir)?;

        let manager = ServiceManager::local_computer(
            None::<&str>,
            ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
        )
        .context("connect to Windows SCM")?;
        let info = ServiceInfo {
            name: OsString::from(SERVICE_NAME),
            display_name: OsString::from(DISPLAY_NAME),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: target_exe.clone(),
            launch_arguments: vec![
                OsString::from("run"),
                OsString::from("--service"),
                OsString::from("--listen"),
                OsString::from(DEFAULT_LISTEN),
                OsString::from("--data-dir"),
                data_dir.as_os_str().to_owned(),
            ],
            dependencies: vec![],
            account_name: Some(OsString::from(r"NT AUTHORITY\LocalService")),
            account_password: None,
        };
        let service = manager
            .create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::START)
            .context("create WakeBridge Windows Service")?;
        service
            .set_description("Multi-site Wake-on-LAN management service")
            .context("set WakeBridge service description")?;
        println!("Installed {SERVICE_NAME} as Automatic / LocalService");
        println!("Binary: {}", target_exe.display());
        println!("Data: {}", data_dir.display());
        Ok(())
    }

    pub fn uninstall() -> Result<()> {
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
            .context("connect to Windows SCM")?;
        let service = manager
            .open_service(
                SERVICE_NAME,
                ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
            )
            .context("open WakeBridge service")?;
        let _ = service.stop();
        service.delete().context("delete WakeBridge service")?;
        println!("Uninstalled {SERVICE_NAME}; data directory was preserved.");
        Ok(())
    }

    pub fn start() -> Result<()> {
        let service = open_service(ServiceAccess::START | ServiceAccess::QUERY_STATUS)?;
        let arguments: [OsString; 0] = [];
        service
            .start(&arguments)
            .context("start WakeBridge service")?;
        println!("Started {SERVICE_NAME}");
        Ok(())
    }

    pub fn stop() -> Result<()> {
        let service = open_service(ServiceAccess::STOP | ServiceAccess::QUERY_STATUS)?;
        let _ = service.stop().context("stop WakeBridge service")?;
        println!("Stop requested for {SERVICE_NAME}");
        Ok(())
    }

    pub fn status() -> Result<()> {
        let service = open_service(ServiceAccess::QUERY_STATUS)?;
        let status = service.query_status().context("query WakeBridge service")?;
        println!("{}: {:?}", SERVICE_NAME, status.current_state);
        Ok(())
    }

    fn open_service(access: ServiceAccess) -> Result<windows_service::service::Service> {
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
            .context("connect to Windows SCM")?;
        manager
            .open_service(SERVICE_NAME, access)
            .context("open WakeBridge service")
    }

    fn grant_data_directory(path: &PathBuf) -> Result<()> {
        let permission = r"NT AUTHORITY\LOCAL SERVICE:(OI)(CI)M";
        let status = std::process::Command::new("icacls")
            .arg(path)
            .arg("/grant")
            .arg(permission)
            .arg("/inheritance:e")
            .status()
            .context("grant LocalService data directory permission")?;
        if !status.success() {
            bail!("icacls failed with {}", status);
        }
        Ok(())
    }

    fn service_error() -> windows_service::Error {
        windows_service::Error::Winapi(std::io::Error::other("WakeBridge service runtime error"))
    }
}

#[cfg(not(windows))]
mod windows_impl {
    use super::*;

    pub fn run_service() -> Result<()> {
        bail!("Windows Service commands are only available on Windows")
    }

    pub fn install() -> Result<()> {
        bail!("Windows Service commands are only available on Windows")
    }

    pub fn uninstall() -> Result<()> {
        bail!("Windows Service commands are only available on Windows")
    }

    pub fn start() -> Result<()> {
        bail!("Windows Service commands are only available on Windows")
    }

    pub fn stop() -> Result<()> {
        bail!("Windows Service commands are only available on Windows")
    }

    pub fn status() -> Result<()> {
        bail!("Windows Service commands are only available on Windows")
    }
}

pub use windows_impl::{install, run_service, start, status, stop, uninstall};
