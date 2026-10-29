//! Platform-specific pieces behind portable functions: host and user names,
//! shared memory and descriptor passing, and later file watching and SIGBUS
//! handling.

pub mod atomic;
pub mod browser;
#[cfg(unix)]
pub mod fdpass;
pub mod files;
pub mod interrupt;
pub mod paths;
#[cfg(unix)]
pub mod process;
pub mod settings;
pub mod shm;

/// This machine's host name, used to keep one log stream per machine.
pub fn hostname() -> String {
    #[cfg(target_os = "linux")]
    if let Ok(name) = std::fs::read_to_string("/proc/sys/kernel/hostname") {
        let name = name.trim();
        if !name.is_empty() {
            return name.to_string();
        }
    }
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// The login name of the current user.
pub fn username() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// The current time as RFC 3339 UTC with milliseconds.
pub fn now_rfc3339() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format_rfc3339(now.as_millis())
}

/// Format milliseconds since the Unix epoch as RFC 3339 UTC.
pub fn format_rfc3339(millis: u128) -> String {
    let secs = millis / 1000;
    let ms = millis % 1000;
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{ms:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Identity for this process lifetime, shared by retained jobs and their readers.
pub fn process_instance() -> &'static str {
    static INSTANCE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    INSTANCE.get_or_init(|| format!("{}:{}", std::process::id(), now_rfc3339()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn rfc3339() {
        assert_eq!(super::format_rfc3339(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            super::format_rfc3339(1_791_623_643_512),
            "2026-10-10T09:14:03.512Z"
        );
        assert_eq!(
            super::format_rfc3339(951_782_400_000),
            "2000-02-29T00:00:00.000Z"
        );
    }
}
