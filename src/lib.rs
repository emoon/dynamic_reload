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
use std::fmt::Write;


pub struct Lib {
    pub lib: Library,
    pub loaded_path: PathBuf,
    pub original_path: Option<PathBuf>,
}

pub struct DynamicReload {
    libs: Vec<Rc<Lib>>,
    watcher: Option<RecommendedWatcher>,
    shadow_dir: Option<TempDir>,
    search_paths: Vec<&'static str>,
    watch_recv: Receiver<Event>,
}

pub enum Search {
    Default,
    Backwards,
}

#[derive(PartialEq)]
pub enum UsePlatformName {
    No,
    Yes,
}

// Test

impl DynamicReload {
    ///
    ///
    pub fn new(search_paths: Option<Vec<&'static str>>,
               shadow_dir: Option<&'static str>,
               _search: Search)
               -> DynamicReload {
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
    /// If UsePlatformName is set to Yes the input name will be formatted according
    /// to the standard way libraries looks on that platform examples:
    ///
    /// ```ignore
    /// Windows: foobar -> foobar.dll
    /// Linux:   foobar -> libfoobar.so
    /// Mac:     foobar -> libfoobar.dylib
    /// ````
    ///
    /// If set to no the given inputname will be used as is. This function
    /// will also search for the file in this priority order
    ///
    ///
    /// ```ignore
    /// 1. Current directory
    /// 2. In the search paths (relative to currect directory) 
    /// 3. Currect directory of the executable 
    /// 4. Serach backwards from executable if Backwards has been set in [new](struct.DynamicReload.html#method.new) 
    /// ```
    ///
    pub fn add_library(&mut self,
                       name: &str,
                       name_format: UsePlatformName)
                       -> Result<Rc<Lib>, String> {
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
    /// Update the will check if a dynamic library needs to be reloaded and if that is the case
    /// then the supplied callback functions will be called
    ///
    /// First the before_reload code. This allows the calling application to performe actions
    /// before the dynamic library is unloaded. This can for example be to save some internal
    /// state that needs to be restorted when relodaded
    ///
    /// After the reloading is comple the post_reload function will be called giving the host
    /// application the possibility to restore any potentially saved state
    ///
    /// If no callbacks are needed use the regular [update](struct.DynamicReload.html#method.update) call instead
    ///
    pub fn update_with_callback<F, T>(&mut self,
                                      ref update_call: F,
                                      data: &mut T)
        where F: Fn(&mut T, bool, &Rc<Lib>)
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

    ///
    /// Updates the DynamicReload handler and reloads the dynamic libraries if needed
    ///
    ///
    pub fn update(&self) {}

    fn reload_libs<F, T>(&mut self,
                         file_path: &PathBuf,
                         ref update_call: F,
                         data: &mut T)
        where F: Fn(&mut T, bool, &Rc<Lib>)
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
        where F: Fn(&mut T, bool, &Rc<Lib>)
    {
        update_call(data, true, &self.libs[index]);
        self.libs.swap_remove(index);

        match Self::load_library(self, file_path) {
            Ok(lib) => {
                self.libs.push(lib.clone());
                update_call(data, false, &lib);
            }

            // What should we really do here?
            Err(err) => {
                println!("Unable to reload lib {:?} err {:?}", file_path, err);
            }
        }
    }


    fn try_load_library(&self,
                        name: &str,
                        name_format: UsePlatformName)
                        -> Result<Rc<Lib>, String> {
        if let Some(path) = Self::search_dirs(self, name, name_format) {
            Self::load_library(self, &path)
        } else {
            let mut t = "Unable to find ".to_string();
            t.push_str(name);
            Err(t)
        }
    }


    fn load_library(&self, full_path: &PathBuf) -> Result<Rc<Lib>, String> {
        let path;
        let original_path;

        if let Some(sd) = self.shadow_dir.as_ref() {
            path = sd.path().join(full_path.file_name().unwrap());
            if !Self::try_copy(&full_path, &path) {
                let mut error = "".to_string();
                write!(error, "Unable to copy {:?} to {:?}", full_path, path).unwrap();
                return Err(error);
            }
            original_path = Some(full_path.clone());
        } else {
            original_path = None;
            path = full_path.clone();
        }

        Self::init_library(original_path, path)
    }

    fn init_library(org_path: Option<PathBuf>, path: PathBuf) -> Result<Rc<Lib>, String> {
        match Library::new(&path) {
            Ok(l) => {
                Ok(Rc::new(Lib {
                    original_path: org_path,
                    loaded_path: path,
                    lib: l,
                }))
            }
            Err(e) => {
                let mut error = "".to_string();
                write!(error, "Unable to load library {:?}", e).unwrap();
                Err(error)
            }
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

    fn search_dirs(&self, name: &str, name_format: UsePlatformName) -> Option<PathBuf> {
        let lib_name = Self::get_library_name(name, name_format);

        // 1. Serach the current directory
        if let Some(path) = Self::search_current_dir(&lib_name) {
            return Some(path);
        }

        // 2. search the relative paths
        if let Some(path) = Self::search_relative_paths(self, &lib_name) {
            return Some(path);
        }

        // 3. searches in the executable dir and then backwards
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

    // In some cases when a file has been set that it's reloaded it's actually not possible
    // to read from it directly so this code does some testing first to ensure we
    // can actually read from it (by using metadata which does a stat on the file)
    // If we can't read from it we wait for 100 ms before we try again, if we can't
    // do it with in 1 sec we give up
    //
    fn try_copy(src: &Path, dest: &Path) -> bool {
        for _ in 0..10 {
            match fs::metadata(src) {
                Ok(file) => {
                    let len = file.len();
                    if len > 0 {
                        fs::copy(&src, &dest).unwrap();
                        // println!("Copy from {} {}", src.to_str().unwrap(), dest.to_str().unwrap());
                        return true;
                    }
                }
                _ => (),
            }

            thread::sleep(Duration::from_millis(100));
        }

        false
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

    pub fn get_search_paths(search_paths: Option<Vec<&'static str>>) -> Vec<&'static str> {
        match search_paths {
            Some(paths) => paths.clone(),
            None => Vec::new(),
        }
    }

    fn get_library_name(name: &str, name_format: UsePlatformName) -> String {
        if name_format == UsePlatformName::Yes {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::mpsc::channel;
    use std::path::{Path, PathBuf};
    use std::env;
    use std::thread;
    use std::time::Duration;
    use std::rc::Rc;
    use std::fs;
    use std::sync::{Once, ONCE_INIT};

    static START: Once = ONCE_INIT;

    #[derive(Default)]
    struct TestNotifyCallback {
        update_call_done: bool,
        after_update_done: bool,
    }

    impl TestNotifyCallback {
        fn update_call(&mut self, before: bool, _lib: &Rc<Lib>) {
            if before {
                self.update_call_done = true;
            } else {
                self.after_update_done = true;
            }
        }
    }

    fn compile_test_shared_lib() -> PathBuf {
        let exe_path = env::current_exe().unwrap();
        let lib_path = exe_path.parent().unwrap();
        let lib_name = "test_shared";
        let lib_full_path = Path::new(&lib_path).join(DynamicReload::get_dynamiclib_name(lib_name));

        START.call_once(|| {
            Command::new("rustc")
                .arg("src/test_shared.rs")
                .arg("--crate-name")
                .arg(&lib_name)
                .arg("--crate-type")
                .arg("dylib")
                .arg("--out-dir")
                .arg(&lib_path)
                .output()
                .unwrap_or_else(|e| panic!("failed to execute process: {}", e));
        });

        // Make sure file exists
        assert!(DynamicReload::is_file(&lib_full_path).is_some());

        lib_full_path
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
        assert_eq!(DynamicReload::get_library_name("foobar", UsePlatformName::Yes),
                   "libfoobar.dylib");
    }

    #[test]
    fn test_get_library_name() {
        assert_eq!(DynamicReload::get_library_name("foobar", UsePlatformName::No),
                   "foobar");
    }

    #[test]
    fn test_search_backwards_from_file_ok() {
        // While this rely on that we have a Cargo project it should be fine
        assert!(DynamicReload::search_backwards_from_exe(&"Cargo.toml".to_string()).is_some());
    }

    #[test]
    fn test_search_backwards_from_file_fail() {
        assert!(DynamicReload::search_backwards_from_exe(&"_no_such_file".to_string()).is_none());
    }

    #[test]
    fn test_add_library_fail() {
        let mut dr = DynamicReload::new(None, None, Search::Default);
        assert!(dr.add_library("wont_find_this_lib", UsePlatformName::No).is_err());
    }

    #[test]
    fn test_add_shared_lib_ok() {
        compile_test_shared_lib();
        let mut dr = DynamicReload::new(None, None, Search::Default);
        assert!(dr.add_library("test_shared", UsePlatformName::Yes).is_ok());
    }

    #[test]
    fn test_add_shared_lib_search_paths() {
        compile_test_shared_lib();
        let mut dr = DynamicReload::new(Some(vec!["../..", "../test"]), None, Search::Default);
        assert!(dr.add_library("test_shared", UsePlatformName::Yes).is_ok());
    }

    #[test]
    fn test_add_shared_shadow_dir_ok() {
        let dr = DynamicReload::new(None, Some("target/debug"), Search::Default);
        assert!(dr.shadow_dir.is_some());
    }

    #[test]
    fn test_add_shared_update_1() {
        let mut notify_callback = TestNotifyCallback::default();  
        let target_path = compile_test_shared_lib();
        let mut dest_path = Path::new(&target_path).to_path_buf();

        let mut dr = DynamicReload::new(None, Some("target/debug"), Search::Default);

        dest_path.set_file_name("test_file");

        fs::copy(&target_path, &dest_path).unwrap();

        assert!(dr.add_library("test_shared", UsePlatformName::Yes).is_ok());

        for i in 0..10 {
            dr.update_with_callback(TestNotifyCallback::update_call, &mut notify_callback); 

            if i == 2 {
                fs::copy(&dest_path, &target_path).unwrap();
            }

            thread::sleep(Duration::from_millis(50));
        }

        assert!(notify_callback.update_call_done);
        assert!(notify_callback.after_update_done);
    }

}
