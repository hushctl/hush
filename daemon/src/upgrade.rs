use std::io::Write as _;

const REPO: &str = "kushalhalder/hush";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub async fn run() {
    if let Err(e) = do_upgrade().await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn do_upgrade() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = std::env::consts::ARCH;
    let asset_name = format!("hush-{os}-{arch}.tar.gz");

    // Preflight: ensure gh is installed and authenticated.
    if let Err(e) = run_gh(&["--version"]).await {
        return Err(format!(
            "gh CLI not found or not working: {e}\n\
             Install it with: brew install gh\n\
             Then authenticate with: gh auth login"
        )
        .into());
    }

    println!("hush v{CURRENT_VERSION} — checking for updates...");

    // Fetch latest release tag.
    let json = run_gh(&[
        "release", "view",
        "--repo", REPO,
        "--json", "tagName",
    ])
    .await
    .map_err(|e| format!("failed to fetch latest release: {e}"))?;

    let tag: serde_json::Value = serde_json::from_str(&json)?;
    let tag_name = tag["tagName"]
        .as_str()
        .ok_or("unexpected JSON from gh release view")?;
    let latest = tag_name.trim_start_matches('v');

    if latest == CURRENT_VERSION {
        println!("Already up to date (v{CURRENT_VERSION}).");
        return Ok(());
    }

    println!("New version available: v{latest}  (current: v{CURRENT_VERSION})");
    println!("Downloading {asset_name}...");

    // Download the asset into a temp directory.
    let pid = std::process::id();
    let tmpdir = std::env::temp_dir().join(format!("hush-upgrade-{pid}"));
    std::fs::create_dir_all(&tmpdir)?;

    let download_result = run_gh(&[
        "release", "download", tag_name,
        "--repo", REPO,
        "--pattern", &asset_name,
        "--dir", tmpdir.to_str().ok_or("tempdir path is not valid UTF-8")?,
    ])
    .await;

    if let Err(e) = download_result {
        let _ = std::fs::remove_dir_all(&tmpdir);
        return Err(format!("download failed: {e}").into());
    }

    // Extract and atomically replace binaries.
    let tarball = std::fs::File::open(tmpdir.join(&asset_name))?;
    let decoder = flate2::read::GzDecoder::new(tarball);
    let mut archive = tar::Archive::new(decoder);

    let cur_exe = std::env::current_exe()?;
    let bin_dir = cur_exe
        .parent()
        .ok_or("cannot determine binary directory")?;

    let mut updated: Vec<String> = Vec::new();

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_owned();
        let fname = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        if fname != "hush" && fname != "hush-hook" {
            continue;
        }

        let dest = bin_dir.join(&fname);
        let tmp = bin_dir.join(format!(".{fname}.tmp"));

        {
            let mut f = std::fs::File::create(&tmp)?;
            std::io::copy(&mut entry, &mut f)?;
            f.flush()?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        }

        std::fs::rename(&tmp, &dest)?;
        updated.push(dest.display().to_string());
    }

    let _ = std::fs::remove_dir_all(&tmpdir);

    if updated.is_empty() {
        return Err("archive contained no recognised binaries (expected 'hush' and/or 'hush-hook')".into());
    }

    for path in &updated {
        println!("  updated: {path}");
    }
    println!("Upgraded to v{latest}. Restart hush to apply.");
    Ok(())
}

/// Run a `gh` subcommand, return stdout on success or stderr-enriched error on failure.
async fn run_gh(args: &[&str]) -> Result<String, String> {
    let out = tokio::process::Command::new("gh")
        .args(args)
        .output()
        .await
        .map_err(|e| format!("failed to spawn gh: {e}"))?;

    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!("gh exited with status {}", out.status)
        } else {
            stderr
        })
    }
}
