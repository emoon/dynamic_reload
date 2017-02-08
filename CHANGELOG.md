# Changelog

This project follows semantic versioning.

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

