//! dynamic_reload is a cross platform library written in [Rust](https://www.rust-lang.org) that makes it easier to do reloading of shared libraries (dll:s on windows, .so on *nix, .dylib on Mac, etc)
//! The intended use is to allow applications to reload code on the fly without closing down the application when some code changes.
//! This can be seen as a lite version of "live" coding for Rust.
//! It's worth to mention here that reloading of shared libraries isn't limited to libraries written in Rust but can be done in any language that can target shared libraries.
//! A typical scenario can look like this:
//!
//! ```ignore
//! 1. Application Foo starts.
//! 2. Foo loads the shared library Bar.
//! 3. The programmer needs to make some code changes to Bar.
//!    Instead of closing down Foo the programmer does the change, recompiles the code.
//! 4. Foo will detect that Bar has been changed on the disk,
//!    will unload the old version and load the new one.
//! ```
//! dynamic_reload library will not try to solve any stale data hanging around in Foo from Bar.
//! It is up to Foo to make sure all data has been cleaned up before Foo is reloaded.
//! Foo will be getting a callback from dynamic_reload before Bar is reloaded and that allows Foo to take needed action.
//! Then another call will be made after Bar has been reloaded to allow Foo to restore state for Bar if needed.
//!
extern crate libc;
extern crate notify;
extern crate libloading;
extern crate tempdir;

use libloading::Library;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender};
use notify::{RecommendedWatcher, Watcher, Event};
use tempdir::TempDir;
use std::time::Duration;
use std::thread;
use std::fs;
use std::env;

pub use libloading::Symbol;

mod error;
pub use self::error::Error;

pub type Result<T> = std::result::Result<T, Error>;

/// Contains the information for a loaded library.
pub struct Lib {
    /// The actual loaded library. Refer to the libloading documentation on how to use this.
    pub lib: Library,
    /// This is the path from where the library was loaded (which may be in a temporary directory)
    pub loaded_path: PathBuf,
    /// Original location of the file. This is keep so dynamic_reload knows which file to look for
    /// updates in case the library has been changed.
    pub original_path: Option<PathBuf>,
}

/// Contains information about loaded libraries and also tracks search paths and reloading events.
pub struct DynamicReload<'a> {
    libs: Vec<Rc<Lib>>,
    watcher: Option<RecommendedWatcher>,
    shadow_dir: Option<TempDir>,
    search_paths: Vec<&'a str>,
    watch_recv: Receiver<Event>,
}

/// Searching for a shared library can be done in current directory, but can also be allowed to
/// search backwards.
pub enum Search {
    /// Search in current directory only
    Default,
    /// Allow searching in current directory and backwards of parent directories as well
    Backwards,
}

/// This is the states that the callback function supplied to [update](struct.DynamicReload.html#method.update) can be called with.
pub enum UpdateState {
    /// Set when a shared library is about to be reloaded. Gives the application time to save state,
    /// do clean up, etc
    Before,
    /// Called when a library has been reloaded. Allows the application to restore state.
    After,
    /// In case reloading of the library failed (broken file, etc) this will be set and allow the
    /// application to to deal with the issue.
    ReloadFailed(Error),
}

/// This is used to decide how the name used for [add_library](struct.DynamicReload.html#method.add_library) is to be handled.
#[derive(PartialEq)]
pub enum PlatformName {
    /// Leave name as is and don't do any formating.
    No,
    /// Format the name according to standard shared library name on the platform.
    ///
    /// ```ignore
    /// Windows: foobar -> foobar.dll
    /// Linux:   foobar -> libfoobar.so
    /// Mac:     foobar -> libfoobar.dylib
    /// ```
    Yes,
}

impl<'a> DynamicReload<'a> {
    ///
    /// Creates a DynamicReload object.
    ///
    /// ```search_path``` is a list of extra paths that when
    /// calling [add_library](struct.DynamicReload.html#method.add_library) the code will
    /// also try to find the shared library within those locations.
    ///
    /// ```shadow_dir``` is a location where a temporary directory will be created to
    /// keep a copy of all the shared libraries and load from there. The reason is that some
    /// operating systems locks loaded shared files which would make it impossible to update them.
    /// By having a separate directory DynamicReload will look for changes in the original path
    /// while having them loaded from another
    ///
    /// ```search``` This is to allow DynamicReload to search in parent directiors from the
    /// executable. Set this to ```Search::Backwards``` to allow that or to ```Search::Default```
    /// to only allow seach in the currenty directory of the of the executable
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // No extra search paths, temp directory in target/debug, allow search backwards
    /// DynamicReload::new(None, Some("target/debug"), Search::Backwards);
    /// ```
    ///
    /// ```ignore
    /// // "../.." extra search path, temp directory in target/debug, allow search backwards
    /// DynamicReload::new(Some(vec!["../.."]), Some("target/debug"), Search::Backwards);
    /// ```
    ///
    pub fn new(search_paths: Option<Vec<&'a str>>,
               shadow_dir: Option<&'a str>,
               _search: Search)
        -> DynamicReload<'a> {
            let (tx, rx) = channel();
            DynamicReload {
                libs: Vec::new(),
                watcher: Self::get_watcher(tx),
                shadow_dir: Self::get_temp_dir(shadow_dir),
                watch_recv: rx,
                search_paths: Self::get_search_paths(search_paths),
            }
        }

    ///
    /// Add a library to be loaded and to be reloaded once updated.
    /// If PlatformName is set to Yes the input name will be formatted according
    /// to the standard way libraries looks on that platform examples:
    ///
    /// ```ignore
    /// Windows: foobar -> foobar.dll
    /// Linux:   foobar -> libfoobar.so
    /// Mac:     foobar -> libfoobar.dylib
    /// ```
    ///
    /// If set to no the given input name will be used as is. This function
    /// will also search for the file in this priority order
    ///
    /// ```ignore
    /// 1. Current directory
    /// 2. In the search paths (relative to current directory)
    /// 3. Current directory of the executable
    /// 4. Search backwards from executable if Backwards has been set DynamicReload::new
    /// ```
    /// # Examples
    ///
    /// ```ignore
    /// // Add a library named test_lib and format it according to standard platform standard.
    /// add_library("test_lib", PlatformName::Yes)
    /// ```
    ///
    pub fn add_library(&mut self,
                       name: &str,
                       name_format: PlatformName)
        -> Result<Rc<Lib>> {
            match Self::try_load_library(self, name, name_format) {
                Ok(lib) => {
                    if let Some(w) = self.watcher.as_mut() {
                        if let Some(path) = lib.original_path.as_ref() {
                            let _ = w.watch(path);
                        }
                    }
                    // Bump the ref here as we keep one around to keep track of files that needs to be reloaded
                    self.libs.push(lib.clone());
                    Ok(lib)
                }
                Err(e) => Err(e),
            }
        }

    ///
    /// Needs to be called in order to handle reloads of libraries.
    ///
    /// ```update_call``` funcion with its data needs to be supplied to allow the application to
    /// take appropriate action depending on what needs to be done with the loaded library.
    ///
    /// ```ignore
    /// struct Plugins {
    ///     // ...
    /// }
    ///
    /// impl Plugins {
    ///    fn reload_callback(&mut self, state: UpdateState, lib: Option<&Rc<Lib>>) {
    ///        match state {
    ///            UpdateState::Before => // save state, remove from lists, etc, here
    ///            UpdateState::After => // shared lib reloaded, re-add, restore state
    ///            UpdateState::ReloadFailed(Error) => // shared lib failed to reload due to error
    ///        }
    ///    }
    /// }
    ///
    /// fn main() {
    ///     let plugins = Plugins { ... };
    ///     let mut dr = DynamicReload::new(None, Some("target/debug"), Search::Backwards);
    ///     dr.add_library("test_shared", Search::Backwards);
    ///     dr.update(Plugin::reload_callback, &mut plugins);
    /// }
    /// ```
    ///
    pub fn update<F, T>(&mut self, ref update_call: F, data: &mut T) where F: Fn(&mut T, UpdateState, Option<&Rc<Lib>>)
    {
        match self.watch_recv.try_recv() {
            Ok(file) => {
                Self::reload_libs(self,
                                  file.path.as_ref().unwrap(),
                                  update_call,
                                  data)
            }
            _ => (),
        }
    }

    fn reload_libs<F, T>(&mut self,
                         file_path: &PathBuf,
                         ref update_call: F,
                         data: &mut T)
        where F: Fn(&mut T, UpdateState, Option<&Rc<Lib>>)
        {
            let len = self.libs.len();
            for i in (0..len).rev() {
                if Self::should_reload(file_path, &self.libs[i]) {
                    Self::reload_lib(self, i, file_path, update_call, data);
                }
            }
        }

    fn reload_lib<F, T>(&mut self,
                        index: usize,
                        file_path: &PathBuf,
                        ref update_call: F,
                        data: &mut T)
        where F: Fn(&mut T, UpdateState, Option<&Rc<Lib>>)
        {
            update_call(data, UpdateState::Before, Some(&self.libs[index]));
            self.libs.swap_remove(index);

            match Self::load_library(self, file_path) {
                Ok(lib) => {
                    self.libs.push(lib.clone());
                    update_call(data, UpdateState::After, Some(&lib));
                }

                Err(err) => {
                    update_call(data, UpdateState::ReloadFailed(err), None);
                    //println!("Unable to reload lib {:?} err {:?}", file_path, err); // Removed due to move in previous line
                }
            }
        }


    fn try_load_library(&self,
                        name: &str,
                        name_format: PlatformName)
        -> Result<Rc<Lib>> {
            match Self::search_dirs(self, name, name_format) {
                Some(path) => Self::load_library(self, &path),
                None => Err(Error::Find(name.into())),
            }
        }


    fn load_library(&self, full_path: &PathBuf) -> Result<Rc<Lib>> {
        let path;
        let original_path;

        if let Some(sd) = self.shadow_dir.as_ref() {
            path = sd.path().join(full_path.file_name().unwrap());
            try!(Self::try_copy(&full_path, &path));
            original_path = Some(full_path.clone());
        } else {
            original_path = None;
            path = full_path.clone();
        }

        Self::init_library(original_path, path)
    }

    fn init_library(org_path: Option<PathBuf>, path: PathBuf) -> Result<Rc<Lib>> {
        match Library::new(&path) {
            Ok(l) => {
                Ok(Rc::new(Lib {
                    original_path: org_path,
                    loaded_path: path,
                    lib: l,
                }))
            }
            Err(e) => Err(Error::Load(e))
        }
    }

    fn should_reload(reload_path: &PathBuf, lib: &Lib) -> bool {
        if let Some(p) = lib.original_path.as_ref() {
            if reload_path.to_str().unwrap().contains(p.to_str().unwrap()) {
                return true;
            }
        }

        false
    }

    fn search_dirs(&self, name: &str, name_format: PlatformName) -> Option<PathBuf> {
        let lib_name = Self::get_library_name(name, name_format);

        // 1. Search the current directory
        if let Some(path) = Self::search_current_dir(&lib_name) {
            return Some(path);
        }

        // 2. Search the relative paths
        if let Some(path) = Self::search_relative_paths(self, &lib_name) {
            return Some(path);
        }

        // 3. Search the executable dir and then go backwards
        Self::search_backwards_from_exe(&lib_name)
    }

    fn search_current_dir(name: &String) -> Option<PathBuf> {
        Self::is_file(&Path::new(name).to_path_buf())
    }

    fn search_relative_paths(&self, name: &String) -> Option<PathBuf> {
        for p in self.search_paths.iter() {
            let path = Path::new(p).join(name);
            if let Some(file) = Self::is_file(&path) {
                return Some(file);
            }
        }

        None
    }

    fn get_parent_dir(path: &PathBuf) -> Option<PathBuf> {
        match path.parent() {
            Some(p) => Some(p.to_path_buf()),
            _ => None,
        }
    }

    fn search_backwards_from_file(path: &PathBuf, lib_name: &String) -> Option<PathBuf> {
        match Self::get_parent_dir(path) {
            Some(p) => {
                let new_path = Path::new(&p).join(lib_name);
                if Self::is_file(&new_path).is_some() {
                    return Some(new_path);
                }
                Self::search_backwards_from_file(&p, lib_name)
            }
            _ => None,
        }
    }

    fn search_backwards_from_exe(lib_name: &String) -> Option<PathBuf> {
        let exe_path = env::current_exe().unwrap_or(PathBuf::new());
        Self::search_backwards_from_file(&exe_path, lib_name)
    }

    fn get_temp_dir(shadow_dir: Option<&str>) -> Option<TempDir> {
        match shadow_dir {
            Some(dir) => {
                match TempDir::new_in(dir, "shadow_libs") {
                    Ok(td) => Some(td),
                    Err(er) => {
                        println!("Unable to create tempdir {}", er);
                        None
                    }
                }
            }
            _ => None,
        }
    }

    fn is_file(path: &PathBuf) -> Option<PathBuf> {
        match fs::metadata(path) {
            Ok(md) => {
                if md.is_file() {
                    Some(path.clone())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    // In some cases when a file has been set so that it's reloaded, it's actually not possible
    // to read from it directly so this code does some testing first to ensure we
    // can actually read from it (by using metadata which does a stat on the file).
    // If we can't read from it, we wait for 100 ms before we try again, if we can't
    // do it within 1 sec we give up
    //
    fn try_copy(src: &Path, dest: &Path) -> Result<()> {
        for _ in 0..10 {
            if let Ok(file) = fs::metadata(src) {
                let len = file.len();
                if len > 0 {
                    return match fs::copy(&src, &dest) {
                        Ok(_)  => Ok(()),
                        Err(e) => Err(Error::Copy(e, src.to_path_buf(), dest.to_path_buf()))
                    };
                    //println!("Copy from {} {}", src.to_str().unwrap(), dest.to_str().unwrap());
                }
            }

            thread::sleep(Duration::from_millis(100));
        }

        Err(Error::CopyTimeOut(src.to_path_buf(), dest.to_path_buf()))
    }

    fn get_watcher(tx: Sender<Event>) -> Option<RecommendedWatcher> {
        match Watcher::new(tx) {
            Ok(watcher) => Some(watcher),
            Err(e) => {
                println!("Unable to create file watcher, no dynamic reloading will be done, \
                          error: {:?}",
                          e);
                None
            }
        }
    }

    fn get_search_paths(search_paths: Option<Vec<&str>>) -> Vec<&str> {
        match search_paths {
            Some(paths) => paths.clone(),
            None => Vec::new(),
        }
    }

    fn get_library_name(name: &str, name_format: PlatformName) -> String {
        if name_format == PlatformName::Yes {
            Self::get_dynamiclib_name(name)
        } else {
            name.to_string()
        }
    }

    /// Formats dll name on Windows ("test_foo" -> "test_foo.dll")
    #[cfg(target_os="windows")]
    fn get_dynamiclib_name(name: &str) -> String {
        format!("{}.dll", name)
    }

    /// Formats dll name on Mac ("test_foo" -> "libtest_foo.dylib")
    #[cfg(target_os="macos")]
    fn get_dynamiclib_name(name: &str) -> String {
        format!("lib{}.dylib", name)
    }

    /// Formats dll name on *nix ("test_foo" -> "libtest_foo.so")
    #[cfg(any(target_os="linux",
              target_os="freebsd",
              target_os="dragonfly",
              target_os="netbsd",
              target_os="openbsd"))]
        fn get_dynamiclib_name(name: &str) -> String {
            format!("lib{}.so", name)
        }
}

impl PartialEq for Lib {
    fn eq(&self, other: &Lib) -> bool {
        self.original_path == other.original_path
    }

    fn ne(&self, other: &Lib) -> bool {
        self.original_path != other.original_path
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::path::{Path, PathBuf};
    use std::env;
    use std::thread;
    use std::time::Duration;
    use std::rc::Rc;
    use std::fs;

    #[derive(Default)]
    struct TestNotifyCallback {
        update_call_done: bool,
        after_update_done: bool,
        fail_update_done: bool,
    }

    impl TestNotifyCallback {
        fn update_call(&mut self, state: UpdateState, _lib: Option<&Rc<Lib>>) {
            match state {
                UpdateState::Before => self.update_call_done = true,
                UpdateState::After => self.after_update_done  = true,
                UpdateState::ReloadFailed(_) => self.fail_update_done  = true,
            }
        }
    }

    fn get_test_shared_lib() -> PathBuf {
        let exe_path = env::current_exe().unwrap();
        let lib_path = exe_path.parent().unwrap();
        let lib_name = "test_shared";
        Path::new(&lib_path).join(DynamicReload::get_dynamiclib_name(lib_name))
    }

    #[test]
    fn test_search_paths_none() {
        assert_eq!(DynamicReload::get_search_paths(None).len(), 0);
    }

    #[test]
    fn test_search_paths_some() {
        assert_eq!(DynamicReload::get_search_paths(Some(vec!["test", "test"])).len(),
        2);
    }

    #[test]
    fn test_get_watcher() {
        let (tx, _) = channel();
        // We expect this to always work
        assert!(DynamicReload::get_watcher(tx).is_some());
    }

    #[test]
    fn test_get_temp_dir_fail() {
        assert!(DynamicReload::get_temp_dir(Some("_no_such_dir")).is_none());
    }

    #[test]
    fn test_get_temp_dir_none() {
        assert!(DynamicReload::get_temp_dir(None).is_none());
    }

    #[test]
    fn test_get_temp_dir_ok() {
        assert!(DynamicReload::get_temp_dir(Some("")).is_some());
    }

    #[test]
    fn test_is_file_fail() {
        assert!(DynamicReload::is_file(&Path::new("haz_no_file_with_this_name").to_path_buf())
                .is_none());
    }

    #[test]
    fn test_is_file_ok() {
        assert!(DynamicReload::is_file(&env::current_exe().unwrap()).is_some());
    }

    #[test]
    #[cfg(target_os="macos")]
    fn test_get_library_name_mac() {
        assert_eq!(DynamicReload::get_library_name("foobar", PlatformName::Yes),
        "libfoobar.dylib");
    }

    #[test]
    fn test_get_library_name() {
        assert_eq!(DynamicReload::get_library_name("foobar", PlatformName::No),
        "foobar");
    }

    #[test]
    fn test_search_backwards_from_file_ok() {
        // While this relays on having a Cargo project, it should be fine
        assert!(DynamicReload::search_backwards_from_exe(&"Cargo.toml".to_string()).is_some());
    }

    #[test]
    fn test_search_backwards_from_file_fail() {
        assert!(DynamicReload::search_backwards_from_exe(&"_no_such_file".to_string()).is_none());
    }

    #[test]
    fn test_add_library_fail() {
        let mut dr = DynamicReload::new(None, None, Search::Default);
        assert!(dr.add_library("wont_find_this_lib", PlatformName::No).is_err());
    }

    #[test]
    fn test_add_shared_lib_ok() {
        let mut dr = DynamicReload::new(None, None, Search::Default);
        assert!(dr.add_library("test_shared", PlatformName::Yes).is_ok());
    }

    #[test]
    fn test_add_shared_lib_search_paths() {
        let mut dr = DynamicReload::new(Some(vec!["../..", "../test"]), None, Search::Default);
        assert!(dr.add_library("test_shared", PlatformName::Yes).is_ok());
    }

    #[test]
    fn test_add_shared_lib_fail_load() {
        let mut dr = DynamicReload::new(None, None, Search::Default);
        assert!(dr.add_library("Cargo.toml", PlatformName::No).is_err());
    }

    #[test]
    fn test_add_shared_shadow_dir_ok() {
        let dr = DynamicReload::new(None, Some("target/debug"), Search::Default);
        assert!(dr.shadow_dir.is_some());
    }

    #[test]
    fn test_add_shared_string_arg_ok() {
        let shadow_dir_string = "target/debug".to_owned();
        let dr = DynamicReload::new(None, Some(&shadow_dir_string), Search::Default);
        assert!(dr.shadow_dir.is_some());
    }

    #[test]
    fn test_add_shared_lib_search_paths_strings() {
        let path1 = "../..".to_owned();
        let path2 = "../test".to_owned();
        let mut dr = DynamicReload::new(Some(vec![&path1, &path2]), None, Search::Default);
        assert!(dr.add_library("test_shared", PlatformName::Yes).is_ok());
    }

    #[test]
    fn test_add_shared_update() {
        let mut notify_callback = TestNotifyCallback::default();
        let target_path = get_test_shared_lib();
        let mut dest_path = Path::new(&target_path).to_path_buf();

        let mut dr = DynamicReload::new(None, Some("target/debug"), Search::Default);

        dest_path.set_file_name("test_file");

        fs::copy(&target_path, &dest_path).unwrap();

        assert!(dr.add_library("test_shared", PlatformName::Yes).is_ok());

        for i in 0..10 {
            dr.update(TestNotifyCallback::update_call, &mut notify_callback);

            if i == 2 {
                fs::copy(&dest_path, &target_path).unwrap();
            }

            thread::sleep(Duration::from_millis(50));
        }

        assert!(notify_callback.update_call_done);
        assert!(notify_callback.after_update_done);
    }

    #[test]
    fn test_add_shared_update_fail_after() {
        let mut notify_callback = TestNotifyCallback::default();
        let target_path = get_test_shared_lib();
        let test_file = DynamicReload::get_dynamiclib_name("test_file_2");
        let mut dest_path = Path::new(&target_path).to_path_buf();

        let mut dr = DynamicReload::new(Some(vec!["target/debug"]), Some("target/debug"), Search::Default);

        assert!(dr.shadow_dir.is_some());

        dest_path.set_file_name(&test_file);

        let _ = DynamicReload::try_copy(&target_path, &dest_path);

        // Wait a while before open the file. Not sure why this is needed.
        thread::sleep(Duration::from_millis(100));

        assert!(dr.add_library(&test_file, PlatformName::No).is_ok());

        for i in 0..10 {
            dr.update(TestNotifyCallback::update_call, &mut notify_callback);

            if i == 2 {
                // Copy a non-shared lib to test the lib handles a broken "lib"
                fs::copy("Cargo.toml", &dest_path).unwrap();
            }

            thread::sleep(Duration::from_millis(50));
        }

        assert_eq!(notify_callback.update_call_done, true);
        assert_eq!(notify_callback.after_update_done, false);
        assert_eq!(notify_callback.fail_update_done, true);
    }

    #[test]
    fn test_lib_equals_true() {
        let mut dr = DynamicReload::new(None, None, Search::Default);
        let lib = dr.add_library("test_shared", PlatformName::Yes).unwrap();
        let lib2 = lib.clone();
        assert!(lib == lib2);
    }

    #[test]
    fn test_lib_equals_false() {
        let mut dr = DynamicReload::new(Some(vec!["target/debug"]), Some("target/debug"), Search::Default);
        let target_path = get_test_shared_lib();

        let test_file = DynamicReload::get_dynamiclib_name("test_file_2");
        let mut dest_path = Path::new(&target_path).to_path_buf();

        dest_path.set_file_name(&test_file);

        let _ = DynamicReload::try_copy(&target_path, &dest_path);
        thread::sleep(Duration::from_millis(100));

        let lib0 = dr.add_library(&test_file, PlatformName::No).unwrap();
        let lib1 = dr.add_library("test_shared", PlatformName::Yes).unwrap();

        assert!(lib0 != lib1);
    }
}
