use crate::model::errors::AppError;
#[cfg(target_os = "macos")]
use crate::model::errors::ErrorSource;

#[cfg(target_os = "macos")]
use smappservice_rs::{AppService, ServiceManagementError, ServiceStatus, ServiceType};

pub fn set_launch_at_login(enabled: bool) -> Result<(), AppError> {
    #[cfg(target_os = "macos")]
    {
        let service = AppService::new(ServiceType::MainApp);

        let result = if enabled {
            service.register().or_else(|error| match error {
                ServiceManagementError::AlreadyRegistered => Ok(()),
                other => Err(other),
            })
        } else {
            service.unregister().or_else(|error| match error {
                ServiceManagementError::JobNotFound => Ok(()),
                other => Err(other),
            })
        };

        return result
            .map_err(|error| AppError::internal(ErrorSource::Settings, error.to_string()));
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = enabled;
        Ok(())
    }
}

pub fn launch_at_login_enabled() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        let service = AppService::new(ServiceType::MainApp);
        let status = service.status();

        return Some(matches!(
            status,
            ServiceStatus::Enabled | ServiceStatus::RequiresApproval
        ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
