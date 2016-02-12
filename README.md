# dynamic_reload [![Build Status](https://travis-ci.org/emoon/dynamic_reload.svg?branch=master)](https://travis-ci.org/emoon/dynamic_reload) [![Build status](https://ci.appveyor.com/api/projects/status/cblu63ub2sqntr9w?svg=true)](https://ci.appveyor.com/project/emoon/dynamic-reload) [![Coverage Status](https://coveralls.io/repos/github/emoon/dynamic_reload/badge.svg?branch=master)](https://coveralls.io/github/emoon/dynamic_reload?branch=master)

dynamic_reload is a cross platform library written in [Rust](https://www.rust-lang.org) that makes it easier to do reloading of shared libraries (dll:s on windows, .so on *nix, .dylib on Mac, etc) The intended use is to allow applications to reload code on the fly without closing down the application when some code changes. This can be seen as a lite version of "live" coding for Rust. It's worth to mention here that reloading of shared libraries isn't limited to libraries written in Rust but can be done in any langague that can target shared libraries. A typical cenario can look like this

```
1. Application Foo starts.
2. Foo loads the shared library Bar.
3. The programmer needs to make some code changes to Bar. Instead of closing down Foo the programmer does the change, recompiles the code.
4. Foo will detect that Bar has been changed on the disk, will unload the old version and load the new one.
```

It's worth to mention that the dynamic_reload library will not try to solve any stale data haninging around in Foo from Bar. It is up to Foo to make sure all data has been cleaned up before Foo is reloaded. Foo will be getting a callback from dynamic_reload before Bar is reloaded and that allows Foo to take needed action. Then another call will be made after Bar has been reloaded to allow Foo to restore state for Bar if needed.


Usage
-----

```toml
# Cargo.toml
[dependencies]
# Lib to go here (not on creates.io yet)
```

Example
-------

```rust
extern crate dynamic_reload;

impl TestNotifyCallback {
	fn update_call(&mut self, before: bool, _lib: &Rc<Lib>) {
		if before {
			self.update_call_done = true;
		} else {
			self.after_update_done = true;
		}
	}
}

fn main() {
	let mut notify_callback = TestNotifyCallback::default();  
	let target_path = compile_test_shared_lib();
	let test_file = DynamicReload::get_dynamiclib_name("test_file_2");
	let mut dest_path = Path::new(&target_path).to_path_buf();

	let mut dr = DynamicReload::new(Some(vec!["target/debug"]), Some("target/debug"), Search::Default);

	assert!(dr.shadow_dir.is_some());

	dest_path.set_file_name(&test_file);

	fs::copy(&target_path, &dest_path).unwrap();

	// Wait a while before open the file. Not sure why this is needed.
	thread::sleep(Duration::from_millis(100));

	assert!(dr.add_library(&test_file, UsePlatformName::No).is_ok());

	for i in 0..10 {
		dr.update(TestNotifyCallback::update_call, &mut notify_callback); 

		if i == 2 {
			// Copy a non-shared lib to test the lib handles a broken "lib"
			fs::copy("Cargo.toml", &dest_path).unwrap();
		}

		thread::sleep(Duration::from_millis(50));
	}
}



```

## Acknowlegment

dynamic_reload uses these two creats for most of the heavy lifting. Thanks!

Notify: https://github.com/passcod/rsnotify
libloading: https://github.com/nagisa/rust_libloading/

## License

Licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

