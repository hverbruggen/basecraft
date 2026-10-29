//! Best-effort new-window browser handoff. Desktop URL portals do not expose
//! a portable new-window option; callers must describe their fallback.
use std::io;

fn browser_command(desktop: &str) -> Option<&'static str> {
    match desktop.trim() {
        "firefox.desktop" | "org.mozilla.firefox.desktop" => Some("firefox"),
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
    let Ok(default) = std::process::Command::new("xdg-settings")
        .args(["get", "default-web-browser"])
        .output()
    else {
        return Ok(false);
    };
    if !default.status.success() {
        return Ok(false);
    }
    let Some(command) = browser_command(&String::from_utf8_lossy(&default.stdout)) else {
        return Ok(false);
    };
    // Neither the desktop identifier nor URL is ever evaluated as shell code.
    let mut child = window_command(command, url).spawn()?;
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
        assert_eq!(
            browser_command("org.mozilla.firefox.desktop\n"),
            Some("firefox")
        );
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
