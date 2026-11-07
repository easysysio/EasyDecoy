# Events

Everything that happens to a decoy is recorded as an **event**: one JSON
object per line, appended to `events_file`. The package writes them to
`/var/log/easydecoy/events.jsonl`. With `events_file` empty they go to stdout,
and EasyDecoy's own log goes to stderr, so stdout carries nothing but events.

```json
{"time":"2026-11-07T11:20:40.409920Z","decoy":"telnet","kind":"login_attempt","src_ip":"10.0.4.17","src_port":61364,"dst_port":23,"username":"admin","password_length":8}
```

Each event is flushed as it is written, so `tail -f` sees it at once and a
crash loses nothing that was already recorded.

## Fields in every event

| Field | What it is |
|---|---|
| `time` | When it happened, UTC, RFC 3339 |
| `decoy` | `telnet` or `http` |
| `kind` | What happened; see below |
| `src_ip` | The client's address. An IPv4 client of a dual-stack listener is shown as plain IPv4. |
| `src_port` | The client's port |
| `dst_port` | The decoy's port |

## Kinds

| `kind` | Decoy | Extra fields |
|---|---|---|
| `connect` | any | none. Every connection starts with one. |
| `login_attempt` | `telnet` | `username`, `password_length` |
| `request` | `http` | `method`, `path`, `host`, `user_agent` |
| `login_attempt` | `http` | as `request`, plus `username`, `password_length` |
| `non_http` | `http` | `sample`: the first 64 bytes the client sent |

A field the client didn't send, such as a request with no `User-Agent`, is
left out rather than recorded as empty.

!!! note "Passwords are never recorded"
    A login attempt keeps the username and the password's **length** only.
    Anyone who reaches a decoy might be a colleague typing their real password
    into the wrong window. The attempt is the signal; the password adds
    nothing worth that risk.

## Client text is cleaned

Everything in an event that came from a client is made safe to store before
it is written:

- Control characters, including newlines and terminal escapes, become `.`,
  so a client cannot forge a log line or recolour your terminal.
- Each field is cut to a fixed length: `method` 16 characters, `username` 128,
  `host` and `user_agent` 256, `path` 512.

## Reading events with jq

Only the login attempts:

```bash
jq -c 'select(.kind == "login_attempt")' /var/log/easydecoy/events.jsonl
```

Who knocks most:

```bash
jq -r .src_ip /var/log/easydecoy/events.jsonl | sort | uniq -c | sort -rn
```

Which usernames are being tried:

```bash
jq -r 'select(.username) | .username' /var/log/easydecoy/events.jsonl \
  | sort | uniq -c | sort -rn
```

What scanners asked the web decoy for:

```bash
jq -r 'select(.kind == "request") | .path' /var/log/easydecoy/events.jsonl \
  | sort | uniq -c | sort -rn | head
```

## Rotation

The package installs `/etc/logrotate.d/easydecoy`, which rotates the events
file **weekly** and keeps **twelve** compressed weeks. It uses `copytruncate`:
EasyDecoy keeps the file open in append mode, so after the copy it carries on
writing at the start of the emptied file, with no restart needed.

To keep events somewhere else as well, send them to a collector over
[syslog](syslog.md).
