# EasyDecoy

HoneyPot for your network.

EasyDecoy runs decoy network services: a Telnet login, a web admin panel, an
FTP server and an SSH server that look real but lead nowhere. Nobody
legitimate has a reason to connect to them, so every connection is worth a look. Each one is recorded as a JSON
event.

The decoys are low-interaction: they imitate a service's prompts and replies,
but nothing a client sends is ever executed or passed to the host. Login
attempts are recorded with the username and the password's *length*, never
the password itself.

## Install

Packages for x86_64 and arm64 come from the signed EasySYS repository, so
upgrades arrive with the rest of the host's updates.

Debian / Ubuntu:

```sh
curl -fsSL https://repo.easysys.io/easydecoy/stable/debian/key.gpg \
  | sudo gpg --dearmor -o /usr/share/keyrings/easysys.gpg
echo "deb [signed-by=/usr/share/keyrings/easysys.gpg] https://repo.easysys.io/easydecoy/stable/debian ./" \
  | sudo tee /etc/apt/sources.list.d/easydecoy.list
sudo apt update && sudo apt install easydecoy
sudo systemctl enable --now easydecoy
```

RHEL / Fedora:

```sh
sudo tee /etc/yum.repos.d/easydecoy.repo >/dev/null <<'EOF'
[easydecoy]
name=EasyDecoy
baseurl=https://repo.easysys.io/easydecoy/stable/redhat
enabled=1
gpgcheck=1
gpgkey=https://repo.easysys.io/easydecoy/stable/redhat/key.gpg
EOF
sudo dnf install easydecoy
sudo systemctl enable --now easydecoy
```

openSUSE, and installing a downloaded package by hand, are in the
[installation guide](https://easydecoy.easysys.io/install/). Each
[release](https://github.com/easysysio/EasyDecoy/releases) also has the
`.deb` and `.rpm` packages.

The decoys and everything else are managed in a web GUI, which listens on the
host itself only (`127.0.0.1:9443`). From your computer:

```sh
ssh -L 9443:127.0.0.1:9443 decoy-host
```

then open <https://localhost:9443/> and create the admin account. The packaged
service puts the decoys on the real ports (23, 80 and 21). The SSH decoy is
set up on 22 but starts off, since that is normally the host's own sshd: move
sshd to another port first, as
[the docs](https://easydecoy.easysys.io/decoys/#port-22) describe. It runs as its own
unprivileged `easydecoy` account, with only the capability to bind ports below
1024. An upgrade keeps your settings and restarts a running service; an
upgrade from 0.1.0 moves the settings in its config file into the database.

## Quick start (from source)

```sh
cargo run
```

Open <https://127.0.0.1:9443/>, create the account, and knock from another
terminal. The decoys listen on `0.0.0.0:2323` (Telnet), `0.0.0.0:8080`
(HTTP) and `0.0.0.0:2121` (FTP), and on `0.0.0.0:2222` (SSH) once you turn it
on under **Decoys**:

```sh
telnet localhost 2323
curl -d 'username=admin&password=x' http://localhost:8080/login
curl ftp://admin:x@localhost:2121/
ssh -p 2222 admin@localhost
```

## The GUI

- **Dashboard**: events, login attempts and sources over 24 hours, 7 or 30
  days, a timeline, the top sources, usernames and paths, and each decoy's state.
- **Events**: every event, filtered by decoy, kind, source or text, and
  exported as JSON lines.
- **Decoys**: Telnet, HTTP, FTP and SSH: ports, banners, hostname, Server
  header, page title, SSH version. A save restarts that decoy, with no service restart.
- **Settings**: limits, retention, an optional events file, syslog.
- **Account**: password, and an audit log of every sign-in and change.

Everything is kept in one SQLite database (`/var/lib/easydecoy/easydecoy.db`
under the service).

## Events

One JSON object per line:

```json
{"time":"2026-11-07T11:20:40.409920Z","decoy":"telnet","kind":"login_attempt","src_ip":"10.0.0.23","src_port":61364,"dst_port":2323,"username":"root","password_length":4}
```

| decoy    | kind            | extra fields                                          |
|----------|-----------------|-------------------------------------------------------|
| any      | `connect`       | —                                                     |
| `telnet` | `login_attempt` | `username`, `password_length`                         |
| `http`   | `request`       | `method`, `path`, `host`, `user_agent`                |
| `http`   | `login_attempt` | as `request`, plus `username`, `password_length`      |
| `http`   | `non_http`      | `sample`: the start of what the client sent           |
| `ftp`    | `login_attempt` | `username`, `password_length`                         |
| `ftp`    | `command`       | `command`, `argument`: sent without a login           |
| `ssh`    | `handshake`     | `client`: the client's version line                   |
| `ssh`    | `login_attempt` | `username`, `method`, `client`, and `password_length` or `key_type`, `key_fingerprint` |

## Syslog

To send every event to a collector such as EasyLog as well, turn on syslog
under **Settings** in the GUI: the collector's `host:port`, UDP or TCP, RFC 5424
or RFC 3164.

Each event becomes one syslog message whose text is its JSON line. Login
attempts go at severity *warning*, everything else at *notice*. In the
default RFC 5424 format, the event's kind is also the message ID:

```
<132>1 2026-11-07T11:20:40.409Z sensor1 easydecoy - login_attempt - {"time":…}
```

Events are always stored locally. A collector that is down costs only the
messages sent while it was down; EasyDecoy reports that in its own log and
carries on.

## Configuration

A small TOML file, read from `EASYDECOY_CONFIG` (default:
`config/easydecoy.toml`), holds what is needed before the GUI exists: the log
level, the database path, and the GUI's address and certificate. See
[config/easydecoy.toml](config/easydecoy.toml). Everything else is in the GUI.

Documentation: <https://easydecoy.easysys.io>

## License

MIT
