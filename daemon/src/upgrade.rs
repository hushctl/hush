use std::io::Write as _;

const REPO: &str = "kushalhalder/hush";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(serde::Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(serde::Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

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
    // Rust reports "aarch64" on Apple Silicon, "x86_64" on Intel — matches our asset names
    let arch = std::env::consts::ARCH;
    let asset_name = format!("hush-{os}-{arch}.tar.gz");

    println!("hush v{CURRENT_VERSION} — checking for updates...");

    let client = reqwest::Client::builder()
        .user_agent(format!("hush/{CURRENT_VERSION}"))
        .build()?;

    let release: Release = client
        .get(format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let latest = release.tag_name.trim_start_matches('v');

    if latest == CURRENT_VERSION {
        println!("Already up to date (v{CURRENT_VERSION}).");
        return Ok(());
    }

    println!("New version available: v{latest}  (current: v{CURRENT_VERSION})");

    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .ok_or_else(|| {
            format!("No asset '{asset_name}' in release v{latest}. Check https://github.com/{REPO}/releases/tag/{}", release.tag_name)
        })?;

    println!("Downloading {}...", asset.name);

    let bytes = client
        .get(&asset.browser_download_url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    let cur_exe = std::env::current_exe()?;
    let bin_dir = cur_exe
        .parent()
        .ok_or("Cannot determine binary directory")?;

    let decoder = flate2::read::GzDecoder::new(std::io::Cursor::new(&bytes));
    let mut archive = tar::Archive::new(decoder);

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

        // Mark executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        }

        // Atomic replace — works on same filesystem, which bin_dir guarantees
        std::fs::rename(&tmp, &dest)?;
        updated.push(dest.display().to_string());
    }

    if updated.is_empty() {
        return Err(format!(
            "Archive contained no recognised binaries (expected 'hush' and/or 'hush-hook')"
        )
        .into());
    }

    for path in &updated {
        println!("  updated: {path}");
    }
    println!("Upgraded to v{latest}. Restart hush to apply.");
    Ok(())
}
