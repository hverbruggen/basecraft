//! Best-effort new-window browser handoff. Desktop URL portals do not expose
//! a portable new-window option; callers must describe their fallback.
use std::io;

fn browser_command(desktop: &str) -> Option<&'static str> {
    match desktop.trim() {
        "firefox.desktop" => Some("firefox"),
        "chromium.desktop" | "chromium-browser.desktop" => Some("chromium"),
        "google-chrome.desktop" => Some("google-chrome"),
        "google-chrome-stable.desktop" => Some("google-chrome-stable"),
        "brave-browser.desktop" => Some("brave-browser"),
        "microsoft-edge.desktop" => Some("microsoft-edge"),
        _ => None,
    }
}

fn window_command(browser: &str, url: &str) -> std::process::Command {
    let mut command = std::process::Command::new(browser);
    command.arg("--new-window").arg(url);
    command
}

// Run only on a worker thread. A stalled desktop helper must not retain the
// browser request forever. Kill and reap this owned child on timeout/error.
fn command_output_with_timeout(
    command: &mut std::process::Command,
    timeout: std::time::Duration,
) -> io::Result<std::process::Output> {
    let mut child = command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output(),
            Ok(None) if start.elapsed() < timeout => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            status => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(status.err().unwrap_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Desktop browser discovery timed out",
                    )
                }));
            }
        }
    }
}

/// Returns false when the platform/default browser has no supported mechanism.
/// Success means the process was started; the browser controls final placement.
pub fn open_new_window(url: &str) -> io::Result<bool> {
    if !(url.starts_with("https://") || url.starts_with("http://"))
        || url.chars().any(char::is_whitespace)
        || url.chars().any(char::is_control)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Expected an HTTP URL",
        ));
    }
    if !cfg!(target_os = "linux") {
        return Ok(false);
    }
    let Ok(default) = command_output_with_timeout(
        std::process::Command::new("xdg-settings").args(["get", "default-web-browser"]),
        std::time::Duration::from_secs(2),
    ) else {
        return Ok(false);
    };
    if !default.status.success() {
        return Ok(false);
    }
    let Some(command) = browser_command(&String::from_utf8_lossy(&default.stdout)) else {
        return Ok(false);
    };
    // Neither the desktop identifier nor URL is ever evaluated as shell code.
    let mut child = match window_command(command, url).spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    // Some browsers stay alive for the lifetime of their window.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_desktop_ids_become_commands() {
        assert_eq!(browser_command("org.mozilla.firefox.desktop\n"), None);
        assert_eq!(browser_command("chromium.desktop"), Some("chromium"));
        assert_eq!(browser_command("custom-wrapper.desktop"), None);
        assert_eq!(browser_command("firefox.desktop; touch /tmp/unsafe"), None);
    }

    #[test]
    fn rejects_non_http_and_control_characters_before_launch() {
        for url in [
            "file:///tmp/query",
            "--new-tab",
            "https://example.org/\narg",
        ] {
            assert_eq!(
                open_new_window(url).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn stalled_discovery_is_reaped_and_returns_timeout() {
        let mut command = std::process::Command::new("sleep");
        command.arg("10");
        let start = std::time::Instant::now();
        let error = command_output_with_timeout(&mut command, std::time::Duration::from_millis(40))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
    }

    #[test]
    fn result_url_remains_one_literal_process_argument() {
        let url =
            "https://blast.ncbi.nlm.nih.gov/Blast.cgi?CMD=Get&RID=ABCD1234567&FORMAT_TYPE=HTML";
        let command = window_command("firefox", url);
        assert_eq!(command.get_program(), "firefox");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["--new-window", url]
        );
    }
}
