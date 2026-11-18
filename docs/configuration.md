# Configuration

EasyDecoy keeps its settings in two places:

- **The GUI** sets everything about the decoys: their ports and names, the
  limits they run under, how long events are kept, and syslog. These settings
  live in the database and take effect as you save them, with no restart.
- **A small config file** says what has to be known before the GUI exists:
  the log level, where the database is, and where the GUI listens.

## Settings in the GUI

### Decoys

| Setting | Default | What it does |
|---|---|---|
| Telnet: Enabled | on | Run the Telnet decoy. |
| Telnet: Listen on | `0.0.0.0:2323` | Address and port, as `ip:port`. The packaged service starts on `0.0.0.0:23`. |
| Telnet: Banner | `Ubuntu 22.04.4 LTS` | Printed when a client connects, as a real host prints `/etc/issue.net`. Empty prints nothing. |
| Telnet: Hostname | `fileserver01` | Shown in the prompt, `fileserver01 login:`. |
| HTTP: Enabled | on | Run the web decoy. |
| HTTP: Listen on | `0.0.0.0:8080` | Address and port. The packaged service starts on `0.0.0.0:80`. |
| HTTP: Server header | `nginx/1.24.0` | Sent as the `Server` response header. Empty leaves it out. |
| HTTP: Page title | `Admin Console` | Title and heading of the sign-in page. |
| FTP: Enabled | on | Run the FTP decoy. An upgrade from 0.1.0 leaves it off. |
| FTP: Listen on | `0.0.0.0:2121` | Address and port. The packaged service starts on `0.0.0.0:21`. |
| FTP: Banner | `(vsFTPd 3.0.5)` | The greeting after `220`, as a real server names itself. |
| SSH: Enabled | off | Run the SSH decoy. Off until you turn it on: see [Port 22](decoys.md#port-22). |
| SSH: Listen on | `0.0.0.0:2222` | Address and port. The packaged service sets `0.0.0.0:22`. |
| SSH: Version | `OpenSSH_8.9p1 Ubuntu-3ubuntu0.10` | Sent after `SSH-2.0-` when a client connects. Plain ASCII, no `-` before the first space. |

Saving a decoy restarts that decoy alone. Its listener is closed and opened
again on the new address, and the sessions it had open end.

### Limits and recording

| Setting | Default | What it does |
|---|---|---|
| Concurrent sessions | `256` | Open sessions across all decoys. Connections beyond it are closed at once. |
| Idle timeout | `30` s | A client that sends nothing for this long is disconnected. |
| Session limit | `120` s | No session lasts longer than this, however active the client. |
| Keep events for | `90` days | Older events are deleted from the database every hour. `0` keeps them forever. |
| Also write events to | empty | A file every event is appended to, as a JSON line. Empty writes no file; `-` writes to stdout. See [Events](events.md#an-events-file). |

Changing a limit restarts every decoy, since they share it.

### Syslog

Off by default. See [Syslog](syslog.md).

| Setting | Default | What it does |
|---|---|---|
| Enabled | off | Send every event to a collector as well. |
| Collector | `127.0.0.1:514` | `host:port`. A name is resolved again for each UDP message. |
| Protocol | UDP | UDP, or TCP (newline-framed). |
| Format | RFC 5424 | RFC 5424, or RFC 3164 for collectors that read nothing else. |
| Facility | `local0` | `user`, `daemon`, `auth`, `authpriv` or `local0` to `local7`. |
| Hostname | empty | The HOSTNAME in each message. Empty uses this machine's name. |

### What a save checks

A save is refused, with the reason shown on the form, when:

- a number is outside its range, such as a session limit of 0,
- an address isn't `ip:port`, or a decoy would take the GUI's port or another
  decoy's,
- a text field holds a line break or is too long,
- the events file can't be opened for writing by the service.

Nothing is stored until the whole form is valid.

## The config file

EasyDecoy reads it from the path in the `EASYDECOY_CONFIG` environment
variable. Without the variable, it reads `config/easydecoy.toml` relative to
the directory it was started in. The packaged service sets the variable to
`/etc/easydecoy/easydecoy.toml`.

```toml
log_level = "info"
db_path   = ""
gui_bind  = "127.0.0.1:9443"
gui_cert  = ""
gui_key   = ""
```

| Key | Default | What it does |
|---|---|---|
| `log_level` | `"info"` | EasyDecoy's own log: `error`, `warn`, `info`, `debug` or `trace`. `RUST_LOG` overrides it. |
| `db_path` | `""` | The database. Empty uses `/var/lib/easydecoy/easydecoy.db` under the packaged service, and `easydecoy.db` in the current directory otherwise. |
| `gui_bind` | `"127.0.0.1:9443"` | Where the GUI listens, as `ip:port`. Always HTTPS. |
| `gui_cert`, `gui_key` | `""` | A certificate and its key, as PEM files, for the GUI. Both empty uses a self-signed one. |

Every key has a default, and a file that doesn't exist runs everything on
defaults. A misspelled key is an error, so EasyDecoy stops at startup rather
than ignore it. After editing, restart the service:

```bash
sudo systemctl restart easydecoy
```

### Where the GUI listens

Leave it on `127.0.0.1` and use an SSH tunnel unless you have a reason not to:

```bash
ssh -L 9443:127.0.0.1:9443 decoy-host
```

If you have a management network the intruders you are watching for can't
reach, its address works too, for example `gui_bind = "10.99.0.12:9443"`. Two
things are refused or flagged:

- A decoy may never listen on the GUI's port. The GUI refuses that save.
- Every page shows a warning while the GUI listens on all addresses
  (`0.0.0.0` or `[::]`) or on an address a decoy also uses. Anyone who finds
  the decoy would find the GUI's sign-in page too.

### The GUI's certificate

With `gui_cert` and `gui_key` empty, EasyDecoy makes a self-signed certificate
on its first start and keeps it in the database, so your browser's exception
holds across restarts and upgrades. It covers `localhost`, `127.0.0.1`, `::1`,
the GUI's own address and the host's name.

To use a certificate of your own, from your internal CA for example:

```toml
gui_cert = "/etc/easydecoy/gui.crt"
gui_key  = "/etc/easydecoy/gui.key"
```

The files are read at each start, so a renewed certificate takes effect on
the next restart. The service account has to be able to read them:

```bash
sudo chgrp easydecoy /etc/easydecoy/gui.key
sudo chmod 640 /etc/easydecoy/gui.key
```

## Ports below 1024

The packaged service is granted `CAP_NET_BIND_SERVICE`, so it binds 23, 80, 21
and 22 without running as root. Running the binary by hand on those ports needs the
same capability:

```bash
sudo setcap cap_net_bind_service=+ep /path/to/easydecoy
```

Don't run it as root to get them.
