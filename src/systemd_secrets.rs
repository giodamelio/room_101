use std::path::Path;
use std::process::Command;
use tracing::{debug, error, warn};

#[derive(Debug, thiserror::Error)]
pub enum SystemdSecretsError {
    #[error("systemd-creds command failed: {0}")]
    CommandFailed(String),
    #[error("systemd-creds not available: {0}")]
    NotAvailable(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, SystemdSecretsError>;

/// Write a secret as an encrypted systemd credential
pub async fn write_secret(name: &str, content: &[u8], path: &str, user_scope: bool) -> Result<()> {
    debug!(
        "Writing systemd secret '{}' to {} (user_scope: {}) - content length: {} bytes",
        name,
        path,
        user_scope,
        content.len()
    );

    // Check if systemd-creds is available before proceeding
    if !is_available() {
        error!(
            "systemd-creds is not available - cannot write secret '{}'",
            name
        );
        return Err(SystemdSecretsError::NotAvailable(
            "systemd-creds command not found. Install systemd >= 248 for credential support."
                .to_string(),
        ));
    }

    // Ensure the directory exists
    if let Some(parent) = Path::new(path).parent() {
        debug!(
            "Creating directory {} if it doesn't exist",
            parent.display()
        );
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            error!("Failed to create directory {}: {}", parent.display(), e);
            return Err(SystemdSecretsError::Io(e));
        } else {
            debug!("Directory {} is ready", parent.display());
        }
    }

    // Build systemd-creds command
    let mut cmd = Command::new("systemd-creds");
    cmd.arg("encrypt")
        .arg("--name")
        .arg(name)
        .arg("-") // Read from stdin
        .arg(path);

    if user_scope {
        cmd.arg("--user");
    }

    // Log the full command being executed
    let cmd_args: Vec<String> = std::iter::once("systemd-creds".to_string())
        .chain(cmd.get_args().map(|arg| arg.to_string_lossy().to_string()))
        .collect();
    debug!("Executing command: {}", cmd_args.join(" "));

    // Execute command with content as stdin
    let output = match cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(mut child) => {
            // Write content to stdin
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                if let Err(e) = stdin.write_all(content) {
                    error!("Failed to write to systemd-creds stdin: {}", e);
                    return Err(SystemdSecretsError::Io(e));
                }
            }

            // Wait for completion
            match child.wait_with_output() {
                Ok(output) => output,
                Err(e) => {
                    error!("Failed to execute systemd-creds: {}", e);
                    return Err(SystemdSecretsError::Io(e));
                }
            }
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                return Err(SystemdSecretsError::NotAvailable(
                    "systemd-creds command not found".to_string(),
                ));
            }
            return Err(SystemdSecretsError::Io(e));
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        error!(
            "systemd-creds encrypt failed for '{}' (exit code: {:?})",
            name,
            output.status.code()
        );
        error!("stderr: {}", stderr);
        if !stdout.is_empty() {
            error!("stdout: {}", stdout);
        }
        if stderr.contains("Permission denied") || stderr.contains("Operation not permitted") {
            return Err(SystemdSecretsError::PermissionDenied(format!(
                "Permission denied writing to {}: {}",
                path, stderr
            )));
        }
        return Err(SystemdSecretsError::CommandFailed(stderr.to_string()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        debug!("systemd-creds stdout: {}", stdout.trim());
    }
    debug!("Successfully wrote systemd secret '{}' to {}", name, path);
    Ok(())
}

/// Delete a systemd credential file
pub async fn delete_secret(name: &str, path: &str, _user_scope: bool) -> Result<()> {
    debug!("Deleting systemd secret '{}' from {}", name, path);

    // Simply remove the file - systemd-creds doesn't have a delete command
    match tokio::fs::remove_file(path).await {
        Ok(()) => {
            debug!("Successfully deleted systemd secret '{}'", name);
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            debug!("Systemd secret '{}' was already deleted", name);
            Ok(()) // Not an error if file doesn't exist
        }
        Err(e) => {
            error!("Failed to delete systemd secret '{}': {}", name, e);
            Err(SystemdSecretsError::Io(e))
        }
    }
}

/// Check if systemd-creds is available on the system
pub fn is_available() -> bool {
    Command::new("systemd-creds")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_is_available() {
        // This test will pass if systemd-creds is installed, otherwise it will show
        // that the functionality gracefully handles missing systemd-creds
        let available = is_available();
        println!("systemd-creds available: {}", available);
    }
}
