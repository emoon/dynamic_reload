extern crate dynamic_reload;

use dynamic_reload::{DynamicReload, Lib, Symbol, Search, PlatformName, UpdateState};
use std::rc::Rc;
use std::time::Duration;
use std::thread;

struct Plugins {
    plugins: Vec<Rc<Lib>>,
}

impl Plugins {
    fn add_plugin(&mut self, plugin: &Rc<Lib>) {
        self.plugins.push(plugin.clone());
    }

    fn unload_plugins(&mut self, lib: &Rc<Lib>) {
        for i in (0..self.plugins.len()).rev() {
            if &self.plugins[i] == lib {
                self.plugins.swap_remove(i);
            }
        }
    }

    fn reload_plugin(&mut self, lib: &Rc<Lib>) {
        Self::add_plugin(self, lib);
    }

    // called when a lib needs to be reloaded.
    fn reload_callback(&mut self, state: UpdateState, lib: Option<&Rc<Lib>>) {
        match state {
            UpdateState::Before => Self::unload_plugins(self, lib.unwrap()),
            UpdateState::After => Self::reload_plugin(self, lib.unwrap()),
            UpdateState::ReloadFailed(_) => println!("Failed to reload"),
        }
    }
}

fn main() {
    let mut plugs = Plugins { plugins: Vec::new() };

    // Setup the reload handler. A temporary directory will be created inside the target/debug
    // where plugins will be loaded from. That is because on some OS:es loading a shared lib
    // will lock the file so we can't overwrite it so this works around that issue.
    let mut reload_handler = DynamicReload::new(Some(vec!["target/debug"]),
                                                Some("target/debug"),
                                                Search::Default);

    // test_shared is generated in build.rs
    match reload_handler.add_library("test_shared", PlatformName::Yes) {
        Ok(lib) => plugs.add_plugin(&lib),
        Err(e) => {
            println!("Unable to load dynamic lib, err {:?}", e);
            return;
        }
    }

    // While this is running (printing a constant number) change return value in file src/test_shared.rs
    // build the project with cargo build and notice that this code will now return the new value
    loop {
        reload_handler.update(Plugins::reload_callback, &mut plugs);

        if plugs.plugins.len() > 0 {
            // In a real program you want to cache the symbol and not do it every time if your
            // application is performance critical
            let fun: Symbol<extern "C" fn() -> i32> = unsafe {
                plugs.plugins[0].lib.get(b"shared_fun\0").unwrap()
            };

            println!("Value {}", fun());
        }

        // Wait for 0.5 sec
        thread::sleep(Duration::from_millis(500));
    }
}
