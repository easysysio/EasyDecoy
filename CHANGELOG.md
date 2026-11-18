# Changelog

All notable changes to EasyDecoy are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] — 2026-11-08

### Added

- FTP decoy: a vsftpd-style server that refuses every login, anonymous
  included, and closes after three. Login attempts are recorded with the
  username and password length; other commands sent without a login (`SITE`,
  `RETR`, `PORT`, ...) are recorded as `command` events. On port 21 under the
  packaged service; an upgrade from 0.1.0 leaves it off.
- Management GUI over HTTPS: a dashboard (events, login attempts, sources, a
  timeline, the top sources, usernames and paths), a filterable event list
  with JSON-lines export, and pages for the decoys, limits, retention, syslog
  and the account. It listens on `127.0.0.1:9443` by default, to be reached
  through an SSH tunnel.
- A SQLite database for settings, events, accounts and the audit trail.
  Events are kept for 90 days by default.
- Settings take effect as they are saved: a decoy restarts alone when its
  settings change, and syslog and the events file switch over without a
  restart.
- A decoy that can't bind its port is shown as not running, with the reason,
  and retried every 30 seconds, instead of stopping EasyDecoy.
- First-run setup of the administrator, bcrypt passwords, 12-hour sessions,
  CSRF tokens, a lockout after 5 failed sign-ins, and an audit log of every
  sign-in and change.
- A self-signed certificate for the GUI, made on the first start, or your own
  through `gui_cert` and `gui_key`.
- A warning on every page while the GUI listens on all addresses or on a
  decoy's address, and no decoy may take the GUI's port.

### Changed

- The config file now holds only `log_level`, `db_path`, `gui_bind`,
  `gui_cert` and `gui_key`. Everything else is set in the GUI.
- Events go to the database. The events file is optional, and an empty path
  now means no file; `-` means stdout.
- Event times are always written to the microsecond.
- `chrono` (unmaintained, RUSTSEC-2026-0543) is replaced by `time`, and the
  database layer is sqlx 0.9 with SQLite from libsqlite3-sys 0.40
  (RUSTSEC-2026-0459). `cargo audit` reports nothing.

### Upgrading from 0.1.0

The package keeps `/etc/easydecoy/easydecoy.toml`. On the first start of
0.2.0, its decoy, limit, syslog and events-file settings are imported into the
database, so the decoys keep their ports and names and events still reach
`/var/log/easydecoy/events.jsonl`. After that they are changed in the GUI; the
old keys in the file are ignored, and the log says so until they are removed.

## [0.1.0] — 2026-11-07

### Added

- Telnet decoy: an Ubuntu-style login prompt that refuses every login.
- HTTP decoy: a web admin sign-in page that refuses every sign-in, with a 404
  for any other path.
- Every connection, request and login attempt recorded as a JSON line, to
  stdout or an events file. Login attempts keep the username and the
  password's length, never the password.
- Events sent to a syslog collector (such as EasyLog) as well, over UDP or
  TCP, in RFC 5424 or RFC 3164. Login attempts go at severity warning.
- Limits on concurrent sessions, idle time and session length.
- `.deb` and `.rpm` packages for x86_64 and arm64, with a hardened systemd unit
  that runs EasyDecoy as its own unprivileged account.
- Documentation site at [easydecoy.easysys.io](https://easydecoy.easysys.io).
