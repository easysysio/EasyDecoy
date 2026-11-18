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
| Open sessions, across all decoys | Concurrent sessions, default 256 |
| Silence before a client is dropped | Idle timeout, default 30 s |
| Longest session | Session limit, default 120 s |
| Telnet or FTP line | 1024 bytes |
| FTP commands per connection | 50 |
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

- The whole filesystem is **read-only** to it, except its database in
  `/var/lib/easydecoy` and `/var/log/easydecoy`.
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

## The management GUI

A honeypot's GUI runs on a machine built to be found and probed, so it is
kept out of the way:

- **It listens on `127.0.0.1` by default**, reached over an SSH tunnel. The
  decoys face the network; the GUI doesn't.
- **A decoy can never take the GUI's port**, and every page warns while the
  GUI listens on all addresses or shares one with a decoy.
- **It is HTTPS only**, with a self-signed certificate made on the first start
  or one you provide.
- **Sign-in is guarded**: bcrypt password hashes, sessions stored only as a
  hash of their token, a CSRF token on every form, a lockout after 5 failed
  sign-ins from one address, and an audit log of every sign-in and change.
  [The GUI page](gui.md#sign-in-and-sessions) has the details.
- **The database is readable by the service account only** (mode 0600).

## What EasyDecoy does not do

- **It is not invisible.** A careful intruder can tell a low-interaction decoy
  from a real service, for example by trying commands it never gets to run.
  Decoys catch the scans, the guesses and the scripts, which is most of what
  moves through a network.
- **It does not block anyone.** It records; acting on what it records is up to
  you or your collector.
- **No encrypted decoys.** There is no HTTPS or SSH decoy yet, and syslog is
  sent as plain UDP or TCP. Keep the path to your collector on a network you
  trust.
- **Events on the host are not tamper-proof.** Anyone with root on the decoy
  host can edit the database or the events file. Sending events to a
  collector keeps a copy out of reach.
