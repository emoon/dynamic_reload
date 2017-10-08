# dynamic_reload [![Build Status](https://travis-ci.org/emoon/dynamic_reload.svg?branch=master)](https://travis-ci.org/emoon/dynamic_reload) [![Build status](https://ci.appveyor.com/api/projects/status/cblu63ub2sqntr9w?svg=true)](https://ci.appveyor.com/project/emoon/dynamic-reload) [![Coverage Status](https://coveralls.io/repos/github/emoon/dynamic_reload/badge.svg?branch=master)](https://coveralls.io/github/emoon/dynamic_reload?branch=master) [![Crates.io](https://img.shields.io/crates/v/dynamic_reload.svg)](https://crates.io/crates/dynamic_reload) [![Documentation](https://docs.rs/dynamic_reload/badge.svg)](https://docs.rs/dynamic_reload)

dynamic_reload is a cross platform library written in [Rust](https://www.rust-lang.org) that makes it easier to do reloading of shared libraries (dll:s on windows, .so on *nix, .dylib on Mac, etc). The intended use is to allow applications to reload code on the fly without closing down the application when some code changes. This can be seen as a lite version of "live" coding for Rust. It's worth to mention here that reloading of shared libraries isn't limited to libraries written in Rust but can be done in any language that can target shared libraries. A typical scenario can look like this:

```
1. Application Foo starts.
2. Foo loads the shared library Bar.
3. The programmer needs to make some code changes to Bar.
   Instead of closing down Foo the programmer does the change, recompiles the code.
4. Foo will detect that Bar has been changed on the disk,
   will unload the old version and load the new one.
```

dynamic_reload library will not try to solve any stale data hanging around in Foo from Bar. It is up to Foo to make sure all data has been cleaned up before Foo is reloaded. Foo will be getting a callback from dynamic_reload before Bar is reloaded and that allows Foo to take needed action. Then another call will be made after Bar has been reloaded to allow Foo to restore state for Bar if needed.

Usage
-----

```toml
# Cargo.toml
[dependencies]
dynamic_reload = "0.3.0"

```

Example
-------

To actually test reloading of this example do the following

```
1, cargo run --example example
2. In another shell change src/test_shared.rs to return another value
3. Run cargo build
4. Notice that the value return in 1. is now changed
```

```rust
extern crate dynamic_reload;

use dynamic_reload::{DynamicReload, Lib, Symbol, Search, PlatformName, UpdateState};
use std::sync::Arc;
use std::time::Duration;
use std::thread;

struct Plugins {
    plugins: Vec<Arc<Lib>>,
}

impl Plugins {
    fn add_plugin(&mut self, plugin: &Arc<Lib>) {
        self.plugins.push(plugin.clone());
    }

    fn unload_plugins(&mut self, lib: &Arc<Lib>) {
        for i in (0..self.plugins.len()).rev() {
            if &self.plugins[i] == lib {
                self.plugins.swap_remove(i);
            }
        }
    }

    fn reload_plugin(&mut self, lib: &Arc<Lib>) {
        Self::add_plugin(self, lib);
    }

    // called when a lib needs to be reloaded.
    fn reload_callback(&mut self, state: UpdateState, lib: Option<&Arc<Lib>>) {
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

    //
    // While this is running (printing a number) change return value in file src/test_shared.rs
    // build the project with cargo build and notice that this code will now return the new value
    //
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
```

## Acknowledgment

dynamic_reload uses these two crates for most of the heavy lifting. Thanks!

Notify: https://github.com/passcod/rsnotify

libloading: https://github.com/nagisa/rust_libloading/

## License

Licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
