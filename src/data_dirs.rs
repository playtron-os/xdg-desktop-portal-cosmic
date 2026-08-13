// SPDX-License-Identifier: GPL-3.0-or-later

//! Lookup of shipped data files through the XDG Base Directory Specification.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Default value of `XDG_DATA_DIRS` from the XDG Base Directory Specification.
const XDG_DATA_DIRS_DEFAULT: &str = "/usr/local/share:/usr/share";

/// Prefix that the running executable was installed under: `<prefix>/libexec/exe` -> `<prefix>`.
fn install_prefix() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = dir.file_name()?;

    if name != OsStr::new("libexec") && name != OsStr::new("bin") {
        return None;
    }

    dir.parent().map(Path::to_path_buf)
}

/// XDG data directories, in search order, from an explicitly-supplied environment.
///
/// `XDG_DATA_HOME` first, then every `XDG_DATA_DIRS` entry — defaulting to the
/// specification's `/usr/local/share:/usr/share` — and finally `<prefix>/share` of the
/// running executable, so an install that is not rooted at `/usr` (a Nix store path, an
/// AppDir) finds its own data with no environment set at all.
fn data_dirs(
    data_home: Option<PathBuf>,
    data_dirs_env: Option<&OsStr>,
    install_prefix: Option<&Path>,
) -> Vec<PathBuf> {
    let data_dirs_env = data_dirs_env
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| OsStr::new(XDG_DATA_DIRS_DEFAULT));

    let candidates = data_home
        .into_iter()
        .chain(std::env::split_paths(data_dirs_env))
        .chain(install_prefix.map(|prefix| prefix.join("share")));

    let mut dirs = Vec::new();
    for dir in candidates {
        if !dir.as_os_str().is_empty() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// First existing `dir/relative` among `dirs`, using `exists` to probe the filesystem.
fn find_in_dirs(
    dirs: &[PathBuf],
    relative: &Path,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    dirs.iter()
        .map(|dir| dir.join(relative))
        .find(|path| exists(path))
}

/// Locate a data file, such as `backgrounds/cosmic/foo.jpg`, in the XDG data directories.
///
/// Returns `None` when no data directory ships the file, so callers degrade instead of
/// handing out a path that cannot be opened.
pub fn find_data_file(relative: impl AsRef<Path>) -> Option<PathBuf> {
    let search_dirs = data_dirs(
        dirs::data_dir(),
        std::env::var_os("XDG_DATA_DIRS").as_deref(),
        install_prefix().as_deref(),
    );

    find_in_dirs(&search_dirs, relative.as_ref(), |path| path.exists())
}

#[cfg(test)]
mod tests {
    use super::{data_dirs, find_in_dirs};
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};

    const WALLPAPER: &str = "backgrounds/cosmic/orion_nebula_nasa_heic0601a.jpg";

    /// With no `XDG_DATA_DIRS` set, the specification default is searched.
    #[test]
    fn data_dirs_falls_back_to_the_spec_default() {
        assert_eq!(
            data_dirs(None, None, None),
            vec![
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share")
            ]
        );
    }

    /// An empty `XDG_DATA_DIRS` is treated as unset, per the specification.
    #[test]
    fn data_dirs_treats_empty_env_as_unset() {
        assert_eq!(
            data_dirs(None, Some(OsStr::new("")), None),
            data_dirs(None, None, None)
        );
    }

    /// Fedora: the wallpaper package installs into `/usr/share`.
    #[test]
    fn finds_wallpaper_on_fhs() {
        let dirs = data_dirs(
            Some(PathBuf::from("/home/u/.local/share")),
            Some(OsStr::new("/usr/local/share:/usr/share")),
            Some(Path::new("/usr")),
        );

        let expected = PathBuf::from("/usr/share").join(WALLPAPER);
        let found = find_in_dirs(&dirs, Path::new(WALLPAPER), |path| path == expected);

        assert_eq!(found, Some(expected));
    }

    /// NixOS: nothing is under `/usr`; the wallpaper lives in a store path on `XDG_DATA_DIRS`.
    #[test]
    fn finds_wallpaper_in_a_store_path() {
        let store = Path::new("/nix/store/aaa-cosmic-wallpapers/share");
        let dirs = data_dirs(
            None,
            Some(OsStr::new(
                "/nix/store/aaa-cosmic-wallpapers/share:/nix/store/bbb-icons/share",
            )),
            None,
        );

        let expected = store.join(WALLPAPER);
        let found = find_in_dirs(&dirs, Path::new(WALLPAPER), |path| path == expected);

        assert_eq!(found, Some(expected));
        assert!(!dirs.contains(&PathBuf::from("/usr/share")));
    }

    /// NixOS with no environment at all: the prefix of the running binary is searched.
    #[test]
    fn finds_wallpaper_relative_to_the_install_prefix() {
        let prefix = Path::new("/nix/store/ccc-xdg-desktop-portal-cosmic");
        let dirs = data_dirs(None, None, Some(prefix));

        let expected = prefix.join("share").join(WALLPAPER);
        let found = find_in_dirs(&dirs, Path::new(WALLPAPER), |path| path == expected);

        assert_eq!(found, Some(expected));
    }

    /// A wallpaper no data directory ships resolves to `None`, not a dangling path.
    #[test]
    fn missing_wallpaper_is_none() {
        let dirs = data_dirs(None, None, None);

        assert_eq!(find_in_dirs(&dirs, Path::new(WALLPAPER), |_| false), None);
    }
}
