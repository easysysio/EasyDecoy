# Changelog

All notable changes to EasyDecoy are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
