# Configuration

Settings live in one TOML file. EasyDecoy reads it from the path in the
`EASYDECOY_CONFIG` environment variable. Without the variable, it reads
`config/easydecoy.toml` relative to the directory it was started in. The
packaged service sets the variable to `/etc/easydecoy/easydecoy.toml`.

After editing, restart the service:

```bash
sudo systemctl restart easydecoy
```

## How the file is read

- **Every setting has a default**, so any of them can be left out. A file
  that doesn't exist at all runs everything on defaults, and the startup log
  says so.
- **A misspelled key is an error.** `enabeld = false` stops EasyDecoy at
  startup instead of silently leaving the decoy on.
- **Choices are checked.** A `protocol` other than `udp` or `tcp`, an unknown
  `facility`, or a syslog `server` without a port stops it at startup too.

## General settings

| Key | Default | What it does |
|---|---|---|
| `log_level` | `"info"` | EasyDecoy's own log: `error`, `warn`, `info`, `debug` or `trace`. `RUST_LOG` overrides it. |
| `events_file` | `""` | File events are appended to, one JSON line each. Empty writes them to stdout. The package sets `/var/log/easydecoy/events.jsonl`. |
| `max_connections` | `256` | Open sessions across all decoys. Connections beyond it are closed at once. |
| `idle_timeout_secs` | `30` | A client that sends nothing for this long is disconnected. |
| `session_limit_secs` | `120` | No session lasts longer than this, however active the client. |

## `[telnet]`

| Key | Default | What it does |
|---|---|---|
| `enabled` | `true` | Run the Telnet decoy. |
| `bind` | `"0.0.0.0:2323"` | Address and port to listen on. The package uses `0.0.0.0:23`. |
| `banner` | `"Ubuntu 22.04.4 LTS"` | Printed when a client connects, as a real host prints `/etc/issue.net`. Empty prints nothing. |
| `hostname` | `"fileserver01"` | Shown in the prompt, `fileserver01 login:`. |

## `[http]`

| Key | Default | What it does |
|---|---|---|
| `enabled` | `true` | Run the web decoy. |
| `bind` | `"0.0.0.0:8080"` | Address and port to listen on. The package uses `0.0.0.0:80`. |
| `server_header` | `"nginx/1.24.0"` | Sent as the `Server` response header. Empty leaves the header out. |
| `title` | `"Admin Console"` | Title and heading of the sign-in page. |

## `[syslog]`

Off by default. See [Syslog](syslog.md) for what is sent and how.

| Key | Default | What it does |
|---|---|---|
| `enabled` | `false` | Send every event to a collector as well. |
| `server` | `"127.0.0.1:514"` | The collector, as `host:port`. A name is resolved again for each UDP message. |
| `protocol` | `"udp"` | `udp`, or `tcp` (newline-framed). |
| `format` | `"rfc5424"` | `rfc5424`, or `rfc3164` for collectors that read nothing else. |
| `facility` | `"local0"` | `user`, `daemon`, `auth`, `authpriv` or `local0` to `local7`. |
| `hostname` | `""` | The HOSTNAME in each message. Empty uses this machine's name. |

## A complete example

```toml
log_level   = "info"
events_file = "/var/log/easydecoy/events.jsonl"

max_connections    = 256
idle_timeout_secs  = 30
session_limit_secs = 120

[telnet]
enabled  = true
bind     = "0.0.0.0:23"
banner   = "Debian GNU/Linux 12"
hostname = "fs-ams-03"

[http]
enabled       = true
bind          = "0.0.0.0:80"
server_header = "Apache/2.4.62 (Debian)"
title         = "Storage Manager"

[syslog]
enabled  = true
server   = "10.0.0.5:514"
protocol = "udp"
```

## Ports below 1024

The packaged service is granted `CAP_NET_BIND_SERVICE`, so it binds 23 and 80
without running as root. Running the binary by hand on those ports needs the
same capability:

```bash
sudo setcap cap_net_bind_service=+ep /path/to/easydecoy
```

Don't run it as root to get them.
