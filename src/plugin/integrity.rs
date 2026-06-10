//! Deterministic content hashing of a plugin tree.
//!
//! The grant store pins what the user APPROVED (the manifest hash); the
//! tree hash pins what is actually INSTALLED. It covers every file the
//! install copies (`.git` excluded, matching `copy_plugin_tree`), so a
//! code-only change that leaves the manifest untouched still produces a
//! different hash. The featured index pins releases to this hash, and
//! `aoe plugin update` uses it to tell "up to date" from "same manifest,
//! different code".

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

/// sha256 over the plugin tree: for each file, the `/`-separated relative
/// path, a NUL, the length, and the bytes, in sorted path order.
/// `sha256:<hex>` like every other hash in the plugin system.
pub fn tree_hash(root: &Path) -> Result<String> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    collect(root, root, &mut files)?;
    files.sort();
    let mut hasher = Sha256::new();
    for (rel, path) in files {
        let bytes = std::fs::read(&path).with_context(|| format!("hashing {}", path.display()))?;
        hasher.update(rel.as_bytes());
        hasher.update([0u8]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(format!(
        "sha256:{}",
        super::grants::hex_encode(&hasher.finalize())
    ))
}

fn collect(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect(root, &path, files)?;
        } else {
            let rel = path
                .strip_prefix(root)
                .expect("entry is under root")
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            files.push((rel, path));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str, contents: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn hash_is_stable_and_ignores_git_dir() {
        let a = tempfile::tempdir().unwrap();
        write(a.path(), "aoe-plugin.toml", "id = \"x\"");
        write(a.path(), "themes/dark.toml", "bg = \"#000\"");
        let b = tempfile::tempdir().unwrap();
        write(b.path(), "aoe-plugin.toml", "id = \"x\"");
        write(b.path(), "themes/dark.toml", "bg = \"#000\"");
        write(b.path(), ".git/HEAD", "ref: refs/heads/main");

        let ha = tree_hash(a.path()).unwrap();
        assert!(ha.starts_with("sha256:"));
        assert_eq!(ha, tree_hash(b.path()).unwrap());
    }

    #[test]
    fn code_change_without_manifest_change_alters_hash() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "aoe-plugin.toml", "id = \"x\"");
        write(dir.path(), "bin/worker", "v1");
        let before = tree_hash(dir.path()).unwrap();
        write(dir.path(), "bin/worker", "v2");
        assert_ne!(before, tree_hash(dir.path()).unwrap());
    }

    #[test]
    fn renaming_a_file_alters_hash_even_with_same_bytes() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.txt", "same");
        let before = tree_hash(dir.path()).unwrap();
        std::fs::rename(dir.path().join("a.txt"), dir.path().join("b.txt")).unwrap();
        assert_ne!(before, tree_hash(dir.path()).unwrap());
    }
}
