//! Whether a newer release is out, from GitHub's latest-release reply.

/// GitHub's description of the latest release of `repository`.
pub fn url(repository: &str) -> String {
    let project = repository
        .trim_end_matches('/')
        .trim_start_matches("https://github.com/");
    format!("https://api.github.com/repos/{project}/releases/latest")
}

/// "Basecraft X is available: URL" when GitHub's latest release is newer
/// than `current`.
pub fn notice(current: &str, response: &str) -> Option<String> {
    let release: serde_json::Value = serde_json::from_str(response).ok()?;
    let tag = release["tag_name"].as_str()?;
    let newer = basecraft_protocol::records::later(tag.trim_start_matches('v'), current);
    newer.then(|| {
        format!(
            "Basecraft {} is available: {}",
            tag.trim_start_matches('v'),
            release["html_url"].as_str().unwrap_or("")
        )
    })
}

/// Whether to ask for the latest release at this start: only when the user
/// opted in and a person is at the window, never in offscreen test or
/// screenshot runs.
pub fn wanted(opted_in: bool, platform: Option<&str>, args: &[String]) -> bool {
    opted_in && platform != Some("offscreen") && !args.iter().any(|arg| arg == "--screenshot")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_opted_in_interactive_start_asks() {
        let plain = ["basecraft".to_owned()];
        assert!(wanted(true, None, &plain));
        assert!(wanted(true, Some("wayland"), &plain));
        assert!(!wanted(false, None, &plain));
        assert!(!wanted(true, Some("offscreen"), &plain));
        let shot = ["basecraft".into(), "--screenshot".into(), "out.png".into()];
        assert!(!wanted(true, None, &shot));
    }

    #[test]
    fn a_newer_release_gives_a_notice_and_others_do_not() {
        assert_eq!(
            url("https://github.com/someone/basecraft"),
            "https://api.github.com/repos/someone/basecraft/releases/latest"
        );
        let release = |tag: &str| {
            serde_json::json!({"tag_name": tag, "html_url": "https://example.org/r"}).to_string()
        };
        assert_eq!(
            notice("0.5.0", &release("v0.6.0")).as_deref(),
            Some("Basecraft 0.6.0 is available: https://example.org/r")
        );
        assert!(notice("0.5.0", &release("0.10.0")).is_some());
        assert_eq!(notice("0.5.0", &release("v0.5.0")), None);
        assert_eq!(notice("0.6.1", &release("v0.6.0")), None);
        assert_eq!(notice("0.5.0", "not json"), None);
        assert_eq!(notice("0.5.0", &release("nightly")), None);
    }
}
