// Copyright 2018-2026 the Deno authors. MIT license.

use std::fs;
use std::path::Path;

use deno_core::anyhow::Context;
use deno_core::anyhow::bail;
use deno_core::error::AnyError;

/// Rename the configuration without parsing it, preserving JSONC comments,
/// formatting, and every Deno configuration option.
pub fn migrate(cwd: &Path) -> Result<i32, AnyError> {
  let configs = ["deno.json", "deno.jsonc"]
    .into_iter()
    .filter(|name| cwd.join(name).exists())
    .collect::<Vec<_>>();
  let source_name = match configs.as_slice() {
    [name] => *name,
    [] => bail!("No deno.json or deno.jsonc found in {}", cwd.display()),
    _ => bail!(
      "Both deno.json and deno.jsonc exist; keep only one before migrating"
    ),
  };

  // Validate every destination before changing any files. In particular, an
  // existing JSONC config must also block migration from a JSON config.
  for name in ["oden.json", "oden.jsonc"] {
    if fs::symlink_metadata(cwd.join(name)).is_ok() {
      bail!("Cannot migrate: {name} already exists");
    }
  }
  let source = cwd.join(source_name);
  if !fs::symlink_metadata(&source)?.is_file() {
    bail!("Cannot migrate: {source_name} must be a regular file");
  }
  let destination_name = source_name.replacen("deno", "oden", 1);
  let destination = cwd.join(&destination_name);
  // A hard link reserves the destination atomically without overwriting it.
  // Both paths are in the same directory (and therefore the same filesystem).
  fs::hard_link(&source, &destination)
    .with_context(|| format!("Failed to create {destination_name}"))?;
  if let Err(error) = fs::remove_file(&source) {
    fs::remove_file(&destination)
      .context("Failed to roll back configuration migration")?;
    return Err(error)
      .with_context(|| format!("Failed to remove {source_name}"));
  }
  log::info!("Migrated {source_name} to {destination_name}");
  Ok(0)
}

#[cfg(test)]
mod tests {
  use test_util::TempDir;

  use super::*;

  #[test]
  fn preserves_config_bytes() {
    for extension in ["json", "jsonc"] {
      let temp = TempDir::new();
      let cwd = temp.path();
      let name = format!("deno.{extension}");
      let content =
        b"{\r\n// comment\r\n\"tasks\": {\"hello\": \"echo hello\"}\r\n}\r\n";
      fs::write(cwd.join(&name), content).unwrap();
      migrate(cwd.as_ref()).unwrap();
      assert!(!cwd.join(name).exists());
      assert_eq!(
        fs::read(cwd.join(format!("oden.{extension}"))).unwrap(),
        content
      );
    }
  }

  #[test]
  fn refuses_conflicts_without_modifying_source() {
    for conflict in ["oden.json", "oden.jsonc", "deno.jsonc"] {
      let temp = TempDir::new();
      let cwd = temp.path();
      fs::write(cwd.join("deno.json"), "{}").unwrap();
      fs::write(cwd.join(conflict), "existing").unwrap();
      assert!(migrate(cwd.as_ref()).is_err());
      assert_eq!(fs::read_to_string(cwd.join("deno.json")).unwrap(), "{}");
      assert_eq!(fs::read_to_string(cwd.join(conflict)).unwrap(), "existing");
    }
  }

  #[test]
  fn refuses_missing_config() {
    let temp = TempDir::new();
    assert!(migrate(temp.path().as_ref()).is_err());
  }
}
