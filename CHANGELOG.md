# Changelog

This project follows semantic versioning.

### v0.9.0 (2023-03-09)

- [changed] - Switched from tempdir to tempfile due to security issues #25 

### v0.8.0 (2022-04-16)

- [changed] - API BREAKAGE: Now `DynamicReload::new` takes an extra parameter which is how often to check for ranges (recommended is 2 sec)
- [changed] - Clippy warnings, editions and various cleanup

### v0.4.0 (2019-08-28)

- [changed] - Use Arc instead of Rc to make create more MT friendly.
- [changed] - Updated the notify & libloading dependencies.
              On Linux if the library file (inode) is deleted then inotify will stop sending events. I think it is better to watch the parent folder.
              Also only reload on CLOSE_WRITE and CREATE events.  (Thanks Robert Gabriel Jakabosky)
- [changed] - Also ignore copy errors so copy can be retried 10 times. Compare filename and extensions, to ignore .dll.exp file changes. (Thanks Robert Gabriel Jakabosky)
- [changed] - Use canonicalize make sure the paths are valid (Thanks Gabriel Dube)
- [changed] - minor clean ups

### v0.2.1 (2017-02-08)

- [changed] Updated libloading to 0.3.0

### v0.2.0 (2016-02-28)

- [changed] All errors now uses a Error enum instead of a string. Thanks to Victor M. Suarez for this PR.

### v0.1.2 (2016-02-21)

- [changed] Various data structures now take ```&str``` instead of ```&'static str``` so String can (optionally) be used

### v0.1.1 (2016-02-19)

- [changed] ```Lib``` now implements PartialEq so comparing ```lib == lib``` now works without using internal data

### v0.1.0 (2016-02-14)

Initial Release

