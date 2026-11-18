use sha2::{Digest, Sha256};
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let archive = PathBuf::from(args.next().ok_or("missing release archive path")?);
    let server = PathBuf::from(args.next().ok_or("missing Dioxus-built server path")?);
    let manifest = args.next().map(PathBuf::from);
    if args.next().is_some() {
        return Err("expected archive, server, and optional dist manifest paths".into());
    }
    if !server.is_file() {
        return Err(format!("Dioxus-built server does not exist: {}", server.display()).into());
    }

    let archive = archive.canonicalize()?;
    let unpacked = tempfile::tempdir()?;
    run_tar(["-xJf".as_ref(), archive.as_os_str(), "-C".as_ref(), unpacked.path().as_os_str()])?;
    let release_dir = fs::read_dir(unpacked.path())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.join("ferrisflow").is_file() && path.join("public").is_dir())
        .ok_or("release archive must contain ferrisflow and public assets")?;

    fs::copy(server, release_dir.join("ferrisflow"))?;
    let updated_archive = archive.with_file_name(format!(
        ".{}.updated",
        archive.file_name().ok_or("invalid archive filename")?.to_string_lossy()
    ));
    run_tar([
        "-cJf".as_ref(),
        updated_archive.as_os_str(),
        "-C".as_ref(),
        unpacked.path().as_os_str(),
        release_dir.file_name().ok_or("invalid release directory")?.as_ref(),
    ])?;
    fs::rename(&updated_archive, &archive)?;

    let digest = Sha256::digest(fs::read(&archive)?);
    let digest_hex = format!("{digest:x}");
    let checksum_path = PathBuf::from(format!("{}.sha256", archive.display()));
    let checksum = format!(
        "{digest_hex} *{}\n",
        archive.file_name().ok_or("invalid archive filename")?.to_string_lossy()
    );
    fs::write(checksum_path, checksum)?;

    if let Some(manifest) = manifest {
        let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&manifest)?)?;
        let archive_name = archive.file_name().ok_or("invalid archive filename")?.to_string_lossy();
        value["artifacts"][archive_name.as_ref()]["checksums"]["sha256"] = digest_hex.into();
        fs::write(manifest, serde_json::to_vec_pretty(&value)?)?;
    }
    Ok(())
}

fn run_tar<I, S>(args: I) -> Result<(), Box<dyn Error>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let status = Command::new("tar").args(args).status()?;
    if !status.success() {
        return Err(format!("tar exited with status {status}").into());
    }
    Ok(())
}
