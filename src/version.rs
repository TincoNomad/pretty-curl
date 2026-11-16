use colored::*;
use serde_json::Value;
use std::time::Duration;

pub fn check_latest_version() -> Result<String, Box<dyn std::error::Error>> {
    let response = ureq::get("https://api.github.com/repos/TincoNomad/pretty-curl/releases/latest")
        // Without a timeout a blocked network hangs `--version` until the OS
        // gives up, which can be minutes.
        .timeout(Duration::from_secs(2))
        .call()?;

    let text = response.into_string()?;
    let json: Value = serde_json::from_str(&text)?;

    if let Some(tag) = json.get("tag_name").and_then(|v| v.as_str()) {
        return Ok(tag.to_string());
    }

    Err("Failed to parse version".into())
}

/// True when `latest` is a higher version than `current`. A leading `v` is
/// ignored; anything unparseable is reported as "not newer", so a broken
/// response never nags the user.
pub fn is_newer(latest: &str, current: &str) -> bool {
    fn parts(version: &str) -> Option<Vec<u64>> {
        let core = version
            .trim()
            .trim_start_matches('v')
            .split(['-', '+'])
            .next()?;
        let mut nums: Vec<u64> = core
            .split('.')
            .map(|n| n.parse().ok())
            .collect::<Option<_>>()?;
        while nums.len() < 3 {
            nums.push(0);
        }
        nums.truncate(3);
        Some(nums)
    }

    match (parts(latest), parts(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

pub fn update_pcurl() {
    println!("{} Updating pcurl...", "🔄".cyan());

    // Pin the installer to the release tag instead of `main`: whoever can push
    // to the branch would otherwise get code run on every user's machine.
    let tag = check_latest_version().unwrap_or_else(|_| format!("v{}", env!("CARGO_PKG_VERSION")));
    let url = format!(
        "https://raw.githubusercontent.com/TincoNomad/pretty-curl/{}/install.sh",
        tag
    );

    match run_installer(&url) {
        Ok(true) => {
            println!("{} Update successful!", "✅".green());
            println!(
                "{} Please restart your terminal or run: {}",
                "→".dimmed(),
                "source ~/.bashrc".cyan()
            );
        }
        Ok(false) => {
            println!("{} Update failed. Please try manually:", "❌".red());
            println!("  {}", format!("curl -fsSL {} | bash", url).cyan());
        }
        Err(e) => {
            println!("{} Failed to run update: {}", "❌".red(), e);
        }
    }
}

/// Downloads the installer and feeds it to bash on stdin. No `sh -c`, so a
/// strange tag name in the URL can never become a shell injection.
fn run_installer(url: &str) -> std::io::Result<bool> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let script = Command::new("curl").args(["-fsSL", url]).output()?;
    if !script.status.success() || script.stdout.is_empty() {
        return Ok(false);
    }

    let mut child = Command::new("bash").stdin(Stdio::piped()).spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(&script.stdout)?;
    }
    Ok(child.wait()?.success())
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("v1.6.0", "1.5.0"));
        assert!(!is_newer("v1.5.0", "1.5.0"));
        assert!(!is_newer("v1.4.9", "1.5.0"));
        // String comparison would get this wrong: "1.10" < "1.9".
        assert!(is_newer("1.10.0", "1.9.0"));
        assert!(!is_newer("1.9.0", "1.10.0"));
        // A different length still compares sensibly.
        assert!(is_newer("2", "1.9.9"));
        assert!(!is_newer("1.5", "1.5.0"));
    }

    #[test]
    fn unparseable_versions_are_not_newer() {
        assert!(!is_newer("garbage", "1.5.0"));
        assert!(!is_newer("1.5.0", "garbage"));
        assert!(!is_newer("", "1.5.0"));
    }

    #[test]
    fn ignores_pre_release_and_build_suffixes() {
        assert!(is_newer("v1.6.0-beta.1", "1.5.0"));
        assert!(!is_newer("1.5.0+build7", "1.5.0"));
    }
}
