//! `hive update` — check for and apply updates from GitHub releases.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::{install, version};

const GITHUB_API: &str = "https://api.github.com/repos/hivemind-ai/the-hive/releases/latest";
const REPO: &str = "hivemind-ai/the-hive";

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
}

/// Run `hive update`. If `check_only` is true, only print versions without downloading.
pub async fn run(check_only: bool) -> Result<()> {
    let current = version::VERSION;
    println!("Current version: {current}");
    println!("Checking for updates...");

    let latest = fetch_latest_version().await?;
    println!("Latest version: {latest}");

    if !is_newer(&latest, current) {
        println!("Already up to date.");
        return Ok(());
    }

    if check_only {
        println!("Update available: v{current} → v{latest}");
        return Ok(());
    }

    let target = version::BUILD_TARGET;
    let tarball_name = format!("hive-{latest}-{target}.tar.gz");
    let url = format!("https://github.com/{REPO}/releases/download/v{latest}/{tarball_name}");

    println!("Downloading {tarball_name}...");
    let tmp = download_tarball(&url, &tarball_name).await?;

    apply_update(&tmp, &latest, target)?;
    println!("Updated to v{latest}. Restart your shell if needed.");
    Ok(())
}

async fn fetch_latest_version() -> Result<String> {
    let client = reqwest::Client::builder()
        .user_agent(version::user_agent())
        .build()?;
    let release: GithubRelease = client
        .get(GITHUB_API)
        .send()
        .await
        .context("fetching latest release")?
        .error_for_status()
        .context("GitHub API error")?
        .json()
        .await
        .context("parsing release JSON")?;

    // Strip leading 'v' from tag_name (e.g. "v0.2.1" → "0.2.1")
    Ok(release.tag_name.trim_start_matches('v').to_string())
}

fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| semver::Version::parse(s).ok();
    match (parse(latest), parse(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

async fn download_tarball(url: &str, name: &str) -> Result<std::path::PathBuf> {
    let tmp = tempfile::tempdir().context("creating temp dir")?;
    let tmp_path = tmp.into_path(); // keep temp dir alive

    let client = reqwest::Client::builder()
        .user_agent(version::user_agent())
        .build()?;
    let bytes = client
        .get(url)
        .send()
        .await
        .context("downloading tarball")?
        .error_for_status()
        .context("download error")?
        .bytes()
        .await
        .context("reading tarball bytes")?;

    let tarball_path = tmp_path.join(name);
    std::fs::write(&tarball_path, &bytes).context("writing tarball")?;

    // Extract
    let tar_gz = std::fs::File::open(&tarball_path).context("opening tarball")?;
    let tar = flate2::read::GzDecoder::new(tar_gz);
    let mut archive = tar::Archive::new(tar);
    archive.unpack(&tmp_path).context("extracting tarball")?;

    Ok(tmp_path)
}

fn apply_update(tmp_dir: &Path, ver: &str, target: &str) -> Result<()> {
    let extracted = tmp_dir.join(format!("hive-{ver}-{target}"));
    let bin_dir = install::hive_bin_dir();
    let docker_dir = install::hive_docker_dir();

    // Update non-self binaries
    for name in &["hive-server", "hive-agent", "app-daemon"] {
        let src = extracted.join(name);
        let dst = bin_dir.join(name);
        std::fs::copy(&src, &dst).with_context(|| format!("updating {name}"))?;
        set_executable(&dst)?;
        println!("  {name}");
    }

    // Update Docker templates
    for name in &["Dockerfile.server", "Dockerfile.agent", "Dockerfile.app"] {
        let src = extracted.join("docker").join(name);
        let dst = docker_dir.join(name);
        std::fs::copy(&src, &dst).with_context(|| format!("updating {name}"))?;
    }
    println!("  Docker templates");

    // Self-replace: write to .new, then atomic rename
    let new_hive = extracted.join("hive");
    let dst_hive = bin_dir.join("hive");
    let tmp_hive = bin_dir.join("hive.new");
    std::fs::copy(&new_hive, &tmp_hive).context("copying new hive binary")?;
    set_executable(&tmp_hive)?;
    std::fs::rename(&tmp_hive, &dst_hive).context("replacing hive binary")?;
    println!("  hive (self)");

    // Update version file
    std::fs::write(install::hive_version_file(), ver).context("updating version file")?;

    Ok(())
}

fn set_executable(path: &Path) -> Result<()> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .with_context(|| format!("setting permissions on {}", path.display()))
}
