//! Reading and writing files.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use crate::error::{Error, Result};

pub fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|e| Error::new(format!("cannot read '{}': {e}", path.display())))
}

/// Write `bytes` to `path`, overwriting it in place so that links,
/// ownership and extended attributes are kept. A new file gets the
/// permissions of `like`. If `path` is a running executable (`ETXTBSY`),
/// a new file is renamed over it instead.
pub fn write(path: &Path, bytes: &[u8], like: &Path) -> Result<()> {
    crate::debug!("writing {}", path.display());
    let fail = |e: std::io::Error| Error::new(format!("cannot write '{}': {e}", path.display()));
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    let permissions = fs::metadata(like).map(|m| m.permissions()).ok();
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(permissions.as_ref().map_or(0o755, |p| p.mode()));
    }
    match options.open(path) {
        Ok(mut f) => f.write_all(bytes).map_err(fail),
        Err(e) if cfg!(target_os = "linux") && e.raw_os_error() == Some(26) => {
            replace(path, bytes, permissions).map_err(fail)
        }
        Err(e) => Err(fail(e)),
    }
}

fn replace(path: &Path, bytes: &[u8], permissions: Option<fs::Permissions>) -> std::io::Result<()> {
    let target = fs::canonicalize(path)?;
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = target.with_file_name(format!(".{name}.patchelf-{}", std::process::id()));
    let result = (|| {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        if let Some(p) = permissions {
            fs::set_permissions(&tmp, p)?;
        }
        fs::rename(&tmp, &target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}
