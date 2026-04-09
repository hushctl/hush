//! `hush trust` — install the local CA into the OS trust store.
//!
//! After running this once per machine, browsers automatically trust every
//! daemon cert signed by the shared CA. No per-connection manual exceptions.

use std::io;
use std::path::Path;

use crate::tls::{fingerprint_pem, load_or_generate_ca};

/// Generate the CA if needed, install it into the OS trust store, and
/// delete the existing leaf cert so the daemon regenerates a CA-signed one
/// on next start.
pub fn install(hush_dir: &Path) -> io::Result<()> {
    let ca = load_or_generate_ca(hush_dir)?;

    // Invalidate any existing leaf cert (may be self-signed from before this
    // feature). Daemon will regenerate it CA-signed on next boot.
    let cert_path = hush_dir.join("tls").join("cert.pem");
    let key_path = hush_dir.join("tls").join("key.pem");
    let _ = std::fs::remove_file(&cert_path);
    let _ = std::fs::remove_file(&key_path);

    do_install(&ca.cert_pem_path)?;

    if let Ok(fp) = fingerprint_pem(&std::fs::read(&ca.cert_pem_path)?) {
        println!("CA fingerprint (SHA-256): {fp}");
    }
    println!(
        "Restart hush (`hush`) to generate a CA-signed leaf cert."
    );
    Ok(())
}

#[cfg(target_os = "macos")]
fn do_install(ca_cert_path: &Path) -> io::Result<()> {
    let login_keychain = dirs::home_dir()
        .map(|h| h.join("Library/Keychains/login.keychain-db"))
        .unwrap_or_else(|| std::path::PathBuf::from("login.keychain-db"));

    println!("Installing Hush CA into login keychain (may prompt for password)...");

    let status = std::process::Command::new("security")
        .args([
            "add-trusted-cert",
            "-r",
            "trustRoot",
            "-k",
            login_keychain.to_str().unwrap_or("login.keychain-db"),
            ca_cert_path.to_str().unwrap(),
        ])
        .status()?;

    if status.success() {
        println!("✓ Hush CA installed. Restart your browser if it was already open.");
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "security add-trusted-cert failed — enter your password in the system dialog",
        ))
    }
}

#[cfg(not(target_os = "macos"))]
fn do_install(ca_cert_path: &Path) -> io::Result<()> {
    println!("Run the following to install the Hush CA on Linux:");
    println!(
        "  sudo cp {} /usr/local/share/ca-certificates/hush-local-ca.crt",
        ca_cert_path.display()
    );
    println!("  sudo update-ca-certificates");
    println!();
    println!("Chrome/Chromium picks up the system store automatically.");
    println!("Firefox requires a separate step — add the cert via about:preferences#privacy.");
    Ok(())
}

/// Print CA paths and an scp command for distributing to another machine.
pub fn export(hush_dir: &Path) {
    let ca_crt = hush_dir.join("tls").join("ca.crt");
    let ca_key = hush_dir.join("tls").join("ca.key");

    if !ca_crt.exists() {
        println!("No CA found. Run `hush trust` first to generate one.");
        return;
    }

    println!("CA cert: {}", ca_crt.display());
    println!("CA key:  {} (keep secret)", ca_key.display());
    println!();
    println!("To share with another machine (e.g. 'studio'):");
    println!(
        "  scp {} {} studio:~/.hush/tls/",
        ca_crt.display(),
        ca_key.display()
    );
    println!("  ssh studio hush trust");
}

/// Remove the Hush CA from the OS trust store.
pub fn uninstall(hush_dir: &Path) -> io::Result<()> {
    do_uninstall(hush_dir)
}

#[cfg(target_os = "macos")]
fn do_uninstall(hush_dir: &Path) -> io::Result<()> {
    let ca_cert_path = hush_dir.join("tls").join("ca.crt");
    if !ca_cert_path.exists() {
        println!("No CA cert found at {} — nothing to remove.", ca_cert_path.display());
        return Ok(());
    }

    let status = std::process::Command::new("security")
        .args(["remove-trusted-cert", "-d", ca_cert_path.to_str().unwrap()])
        .status()?;

    if status.success() {
        println!("✓ Hush CA removed from keychain.");
    } else {
        eprintln!("Note: cert may not have been in the keychain (already removed?).");
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn do_uninstall(_hush_dir: &Path) -> io::Result<()> {
    println!("To remove the Hush CA on Linux:");
    println!("  sudo rm /usr/local/share/ca-certificates/hush-local-ca.crt");
    println!("  sudo update-ca-certificates");
    Ok(())
}
