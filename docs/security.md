# Security model

A decoy is the one service on your network built to be found by an intruder.
EasyDecoy is designed on the assumption that everyone who connects to it is
hostile, and that it should give them nothing to use.

## Low-interaction by design

The decoys imitate a service's prompts and replies, and that is all they do.
There is no shell behind the Telnet login, no web application behind the
sign-in page, and no real service anywhere a client could reach. Nothing a
client sends is executed, interpreted or passed to the host.

## Passwords are never stored

A login attempt records the username and the password's **length**. Anyone who
reaches a decoy might be a colleague typing their real password into the wrong
window. Storing it would turn the decoy into a store of real credentials for
the next intruder to find.

## Hostile input, bounded

| Protection | Limit |
|---|---|
| Open sessions, across all decoys | `max_connections`, default 256 |
| Silence before a client is dropped | `idle_timeout_secs`, default 30 s |
| Longest session | `session_limit_secs`, default 120 s |
| Telnet line | 1024 bytes |
| HTTP request head / body | 8 KiB / 4 KiB |
| Recorded text, per field | 16 to 512 characters |

Connections beyond the session limit are closed at once, so a flood costs a
socket for a moment, never a buffer or a task. Everything recorded from a
client has its control characters replaced, so it cannot forge log lines or
send escape sequences to the terminal of whoever reads them.

EasyDecoy is written in Rust, so the code that parses hostile input is
memory-safe.

## Least privilege

The packaged service runs as its own `easydecoy` account, never as root. Its
one extra permission is binding ports below 1024 (`CAP_NET_BIND_SERVICE`).
systemd then takes away what it has no use for:

- The whole filesystem is **read-only** to it, except `/var/log/easydecoy`.
- No access to home directories or devices, and a private `/tmp`.
- No new privileges, no set-uid, no kernel tunables or modules, no clock,
  hostname or namespace changes.
- Only the system calls an ordinary network service makes, and only IPv4,
  IPv6 and local sockets.
- No writable executable memory.

`systemd-analyze security easydecoy` shows the full list on your host.

!!! note "Containers"
    Some of these settings depend on kernel features that unprivileged
    containers, LXC in particular, don't always provide. If the service fails
    to start inside one, `journalctl -u easydecoy` names the setting.

## What EasyDecoy does not do

- **It is not invisible.** A careful intruder can tell a low-interaction decoy
  from a real service, for example by trying commands it never gets to run.
  Decoys catch the scans, the guesses and the scripts, which is most of what
  moves through a network.
- **It does not block anyone.** It records; acting on what it records is up to
  you or your collector.
- **No encryption.** There is no HTTPS or SSH decoy yet, and syslog is sent as
  plain UDP or TCP. Keep the path to your collector on a network you trust.
- **The events file is not tamper-proof.** Anyone with root on the decoy host
  can edit it. Sending events to a collector keeps a copy out of reach.
