# Events

Everything that happens to a decoy is recorded as an **event**: a JSON object
like this one.

```json
{"time":"2026-11-07T11:20:40.409920Z","decoy":"telnet","kind":"login_attempt","src_ip":"10.0.4.17","src_port":61364,"dst_port":23,"username":"admin","password_length":8}
```

Every event is stored in the **database**, which is what the GUI's
[dashboard and events page](gui.md#the-pages) show, and what **Export**
downloads, one JSON object per line. Events are kept for 90 days by default
(**Settings → Keep events for**).

Each event can also go, as the same JSON line, to:

- an [events file](#an-events-file), for tools that tail one;
- a syslog collector, over [syslog](syslog.md).

## Fields in every event

| Field | What it is |
|---|---|
| `time` | When it happened: UTC, RFC 3339, always to the microsecond |
| `decoy` | `telnet`, `http` or `ftp` |
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
| `login_attempt` | `ftp` | `username`, `password_length` |
| `command` | `ftp` | `command`, and `argument` when there is one: a command sent without a login that an ordinary client wouldn't send |

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
- Each field is cut to a fixed length: `method` and `command` 16 characters,
  `username` 128, `host`, `user_agent` and `argument` 256, `path` 512.

## Reading events with jq

An export from the GUI, or the events file, is one JSON object per line.

Only the login attempts:

```bash
jq -c 'select(.kind == "login_attempt")' easydecoy-events.jsonl
```

Who knocks most:

```bash
jq -r .src_ip easydecoy-events.jsonl | sort | uniq -c | sort -rn
```

Which usernames are being tried:

```bash
jq -r 'select(.username) | .username' easydecoy-events.jsonl \
  | sort | uniq -c | sort -rn
```

What scanners asked the web decoy for:

```bash
jq -r 'select(.kind == "request") | .path' easydecoy-events.jsonl \
  | sort | uniq -c | sort -rn | head
```

## An events file

Set **Settings → Also write events to** to a path, and every event is appended
to that file as well, as it is stored. Under the packaged service, use
`/var/log/easydecoy/events.jsonl`: the service can write there, and the
package rotates it.

```bash
sudo tail -f /var/log/easydecoy/events.jsonl
```

`-` writes the events to stdout instead, with EasyDecoy's own log on stderr,
so stdout carries nothing but events:

```bash
cargo run | jq .
```

Lines are flushed as they are written, so `tail -f` sees them at once.

### Rotation

The package installs `/etc/logrotate.d/easydecoy`, which rotates
`/var/log/easydecoy/events.jsonl` **weekly** and keeps **twelve** compressed
weeks. It uses `copytruncate`: EasyDecoy keeps the file open in append mode,
so after the copy it carries on writing at the start of the emptied file, with
no restart needed. Retention in the database is separate, and set in the GUI.
