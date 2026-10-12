// Copyright 2018-2026 the Deno authors. MIT license.

use std::ffi::OsStr;
use std::fs::File;
use std::fs::OpenOptions;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::path::Prefix;
use std::ptr::null;
use std::ptr::null_mut;

use deno_error::JsErrorBox;
use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::FILE_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_NON_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN_IF;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN_REPARSE_POINT;
use windows_sys::Wdk::Storage::FileSystem::FILE_SYNCHRONOUS_IO_NONALERT;
use windows_sys::Wdk::Storage::FileSystem::NtCreateFile;
use windows_sys::Win32::Foundation::RtlNtStatusToDosError;
use windows_sys::Win32::Foundation::UNICODE_STRING;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
use windows_sys::Win32::Storage::FileSystem::FILE_GENERIC_READ;
use windows_sys::Win32::Storage::FileSystem::FILE_GENERIC_WRITE;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_DELETE;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE;
use windows_sys::Win32::Storage::FileSystem::FILE_TRAVERSE;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK_0;
use windows_sys::Win32::System::Kernel::OBJ_CASE_INSENSITIVE;
use windows_sys::Win32::System::Kernel::OBJ_DONT_REPARSE;

use super::error;
use super::io_error;

const DIRECTORY_ACCESS: u32 =
  FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE;

pub(super) fn open(path: &Path) -> Result<File, JsErrorBox> {
  open_with_directory_hook(path, |_| {})
}

// Keep every validated ancestor alive until the final handle is obtained.
// Denying write/delete sharing pins directories against rename and against
// acquiring the write handle needed to set a reparse point. More importantly,
// descendants are opened relative to those handles, never by re-resolving the
// full path after a separate symlink_metadata check.
#[allow(
  clippy::disallowed_methods,
  reason = "Native KV opens the real filesystem after checking permissions"
)]
fn open_with_directory_hook(
  path: &Path,
  mut directory_opened: impl FnMut(&Path),
) -> Result<File, JsErrorBox> {
  let path = std::path::absolute(path).map_err(io_error)?;
  let mut components = path.components();
  let Some(Component::Prefix(prefix)) = components.next() else {
    return Err(error("KV database requires an absolute Windows path"));
  };
  if !matches!(
    prefix.kind(),
    Prefix::Disk(_)
      | Prefix::VerbatimDisk(_)
      | Prefix::UNC(_, _)
      | Prefix::VerbatimUNC(_, _)
  ) || components.next() != Some(Component::RootDir)
  {
    return Err(error("Unsupported KV database path prefix"));
  }
  let names: Vec<_> = components
    .map(|part| match part {
      Component::Normal(name) => Ok(name),
      _ => Err(error("KV database path must not contain '.' or '..'")),
    })
    .collect::<Result<_, _>>()?;
  if names.is_empty() {
    return Err(error("Invalid KV filename"));
  }
  let mut current = PathBuf::from(prefix.as_os_str());
  current.push(r"\");
  let root = OpenOptions::new()
    .access_mode(DIRECTORY_ACCESS)
    .share_mode(FILE_SHARE_READ)
    .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
    .open(&current)
    .map_err(io_error)?;
  validate(&root, true)?;
  let mut ancestors = vec![root];
  directory_opened(&current);
  for (i, name) in names.iter().enumerate() {
    let directory = i + 1 != names.len();
    let file = open_relative(ancestors.last().unwrap(), name, directory)?;
    validate(&file, directory)?;
    if !directory {
      return Ok(file);
    }
    current.push(name);
    ancestors.push(file);
    directory_opened(&current);
  }
  unreachable!()
}

fn validate(file: &File, directory: bool) -> Result<(), JsErrorBox> {
  let metadata = file.metadata().map_err(io_error)?;
  if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
    return Err(error("KV database path cannot contain reparse points"));
  }
  if directory && !metadata.is_dir() {
    return Err(error("KV database parent must be a directory"));
  }
  if !directory && !metadata.is_file() {
    return Err(error("KV database must be a regular file"));
  }
  Ok(())
}

fn open_relative(
  parent: &File,
  name: &OsStr,
  directory: bool,
) -> Result<File, JsErrorBox> {
  let mut wide: Vec<u16> = name.encode_wide().collect();
  if wide.is_empty() || wide.contains(&0) {
    return Err(error("Invalid KV path component"));
  }
  let length = u16::try_from(wide.len() * 2)
    .map_err(|_| error("KV path component is too long"))?;
  let name = UNICODE_STRING {
    Length: length,
    MaximumLength: length,
    Buffer: wide.as_mut_ptr(),
  };
  let attributes = OBJECT_ATTRIBUTES {
    Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
    RootDirectory: parent.as_raw_handle(),
    ObjectName: &name,
    Attributes: (OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE) as u32,
    SecurityDescriptor: null(),
    SecurityQualityOfService: null(),
  };
  let mut status_block = IO_STATUS_BLOCK {
    Anonymous: IO_STATUS_BLOCK_0 { Status: 0 },
    Information: 0,
  };
  let mut handle = null_mut();
  // SAFETY: parent is live throughout the call. The counted UTF-16 buffer and
  // all input/output structures remain allocated for the synchronous call.
  // No allocation size or EA buffer is supplied. The handle is owned only on
  // success and is transferred once to File below.
  let status = unsafe {
    NtCreateFile(
      &mut handle,
      if directory {
        DIRECTORY_ACCESS
      } else {
        FILE_GENERIC_READ | FILE_GENERIC_WRITE
      },
      &attributes,
      &mut status_block,
      null(),
      FILE_ATTRIBUTE_NORMAL,
      if directory {
        FILE_SHARE_READ
      } else {
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
      },
      if directory { FILE_OPEN } else { FILE_OPEN_IF },
      FILE_SYNCHRONOUS_IO_NONALERT
        | FILE_OPEN_REPARSE_POINT
        | if directory {
          FILE_DIRECTORY_FILE
        } else {
          FILE_NON_DIRECTORY_FILE
        },
      null(),
      0,
    )
  };
  if status < 0 {
    // SAFETY: RtlNtStatusToDosError accepts any NTSTATUS and has no pointers.
    let code = unsafe { RtlNtStatusToDosError(status) };
    return Err(io_error(std::io::Error::from_raw_os_error(code as i32)));
  }
  // SAFETY: successful NtCreateFile returned an owned, synchronous file handle.
  Ok(unsafe { File::from_raw_handle(handle) })
}

#[cfg(test)]
#[allow(
  clippy::disallowed_methods,
  reason = "tests exercise actual directory handles and junctions"
)]
mod tests {
  use std::io::Read;
  use std::io::Write;
  use std::sync::atomic::AtomicU64;
  use std::sync::atomic::Ordering;

  use windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;
  use windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION;
  use windows_sys::Win32::System::IO::DeviceIoControl;
  use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;
  use windows_sys::Win32::System::SystemServices::IO_REPARSE_TAG_MOUNT_POINT;

  use super::*;

  struct Temp(PathBuf);
  impl Temp {
    fn new() -> Self {
      static NEXT: AtomicU64 = AtomicU64::new(0);
      let path = std::env::temp_dir().join(format!(
        "oden-kv-windows-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
          .duration_since(std::time::UNIX_EPOCH)
          .unwrap()
          .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
      ));
      std::fs::create_dir(&path).unwrap();
      Self(path)
    }
  }
  impl Drop for Temp {
    fn drop(&mut self) {
      std::fs::remove_dir_all(&self.0).unwrap();
    }
  }

  fn writable_directory(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
      .write(true)
      .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
      .open(path)
  }

  // Junctions need no symlink privilege, so these regressions also run under
  // ordinary Windows accounts. Build the documented mount-point reparse data
  // directly instead of invoking a shell with filesystem paths.
  fn junction(link: &Path, target: &Path) {
    std::fs::create_dir(link).unwrap();
    let canonical = std::fs::canonicalize(target).unwrap();
    let mut name: Vec<u16> = canonical.as_os_str().encode_wide().collect();
    assert!(name.starts_with(&[92, 92, 63, 92])); // Win32 \\?\ prefix.
    name[1] = 63; // NT \??\ prefix.
    let name_bytes = u16::try_from(name.len() * 2).unwrap();
    let data_length = 8 + name_bytes + 4;
    let mut data = Vec::new();
    data.extend_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
    data.extend_from_slice(&data_length.to_le_bytes());
    data.extend_from_slice(&0_u16.to_le_bytes()); // Reserved.
    data.extend_from_slice(&0_u16.to_le_bytes()); // SubstituteNameOffset.
    data.extend_from_slice(&name_bytes.to_le_bytes());
    data.extend_from_slice(&(name_bytes + 2).to_le_bytes()); // PrintNameOffset.
    data.extend_from_slice(&0_u16.to_le_bytes()); // Empty PrintName.
    for ch in name {
      data.extend_from_slice(&ch.to_le_bytes());
    }
    data.extend_from_slice(&[0; 4]); // Terminators for both names.
    let file = writable_directory(link).unwrap();
    let mut returned = 0;
    // SAFETY: file owns a live synchronous directory handle; data contains a
    // complete mount-point reparse buffer and remains live during the call.
    // No output buffer or asynchronous OVERLAPPED pointer is supplied.
    let ok = unsafe {
      DeviceIoControl(
        file.as_raw_handle(),
        FSCTL_SET_REPARSE_POINT,
        data.as_ptr().cast(),
        data.len() as u32,
        null_mut(),
        0,
        &mut returned,
        null_mut(),
      )
    };
    assert_ne!(ok, 0, "{}", std::io::Error::last_os_error());
  }

  #[test]
  fn regular_and_verbatim_paths_preserve_existing_data() {
    let temp = Temp::new();
    let dir = temp.0.join("store Ω");
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("data.kv");
    let mut first = open(&path).unwrap();
    first.write_all(b"original").unwrap();
    first.sync_all().unwrap();
    // Independent opens must remain compatible with each other's share modes.
    let mut second = open(&path).unwrap();
    let mut bytes = Vec::new();
    second.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"original");
    drop(first);
    drop(second);
    let verbatim = std::fs::canonicalize(&path).unwrap();
    let mut file = open(&verbatim).unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"original");
  }

  #[test]
  fn pinned_ancestors_block_directory_replacement_and_reparse_writes() {
    let temp = Temp::new();
    let allowed = temp.0.join("allowed");
    let nested = allowed.join("nested");
    let outside = temp.0.join("outside");
    let moved = temp.0.join("moved");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("data.kv"), b"outside").unwrap();
    // Establish that the account may write directory reparse metadata before
    // traversal pins this directory, so a failed attack is not an ACL artifact.
    drop(writable_directory(&allowed).unwrap());
    let mut attempted = false;
    let mut file = open_with_directory_hook(&nested.join("data.kv"), |path| {
      if path == nested {
        attempted = true;
        // Check an earlier ancestor, not just the last directory. The attacker
        // would rename it and put a junction at the old authorized path.
        let rename = std::fs::rename(&allowed, &moved).unwrap_err();
        assert!(matches!(rename.raw_os_error(), Some(code)
          if code == ERROR_SHARING_VIOLATION as i32 || code == ERROR_ACCESS_DENIED as i32));
        let write = writable_directory(&allowed).unwrap_err();
        assert_eq!(write.raw_os_error(), Some(ERROR_SHARING_VIOLATION as i32));
      }
    })
    .unwrap();
    assert!(attempted);
    file.write_all(b"inside").unwrap();
    drop(file);
    assert_eq!(std::fs::read(nested.join("data.kv")).unwrap(), b"inside");
    assert_eq!(std::fs::read(outside.join("data.kv")).unwrap(), b"outside");
    // Handles are released after open, so normal directory operations resume.
    std::fs::rename(&allowed, &moved).unwrap();
    junction(&allowed, &outside);
    assert!(open(&allowed.join("data.kv")).is_err());
    assert_eq!(std::fs::read(outside.join("data.kv")).unwrap(), b"outside");
  }

  #[test]
  fn junctions_in_any_component_are_rejected_without_touching_target() {
    let temp = Temp::new();
    let outside = temp.0.join("outside");
    let intermediate = temp.0.join("intermediate");
    let final_link = temp.0.join("final.kv");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("existing.kv"), b"untouched").unwrap();
    junction(&intermediate, &outside);
    junction(&final_link, &outside);
    assert!(open(&intermediate.join("new.kv")).is_err());
    assert!(open(&intermediate.join("existing.kv")).is_err());
    assert!(open(&final_link).is_err());
    assert!(!outside.join("new.kv").exists());
    assert_eq!(
      std::fs::read(outside.join("existing.kv")).unwrap(),
      b"untouched"
    );
  }
}
