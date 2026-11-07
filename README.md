# EasyDecoy

HoneyPot for your network.

EasyDecoy runs decoy network services: a Telnet login and a web admin panel
that look real but lead nowhere. Nobody legitimate has a reason to connect to
them, so every connection is worth a look. Each one is recorded as a JSON
event.

The decoys are low-interaction: they imitate a service's prompts and replies,
but nothing a client sends is ever executed or passed to the host. Login
attempts are recorded with the username and the password's *length*, never
the password itself.

## Install

Each [release](https://github.com/easysysio/EasyDecoy/releases) has `.deb` and
`.rpm` packages for x86_64 and arm64.

```sh
sudo apt install ./easydecoy_<version>-1_amd64.deb     # Debian / Ubuntu
sudo dnf install ./easydecoy-<version>-1.x86_64.rpm    # Fedora / RHEL
```

The package installs a systemd unit but does not start it. Look over
`/etc/easydecoy/easydecoy.toml` first: the packaged config uses the real ports
(23 and 80). Then:

```sh
sudo systemctl enable --now easydecoy
sudo tail -f /var/log/easydecoy/events.jsonl
```

The service runs as its own unprivileged `easydecoy` account, with only the
capability to bind ports below 1024. An upgrade keeps your config and restarts
a running service.

## Quick start (from source)

```sh
cargo run | jq .
```

With the default config this listens on `0.0.0.0:2323` (Telnet) and
`0.0.0.0:8080` (HTTP), and writes events to stdout. EasyDecoy's own log goes
to stderr.

```sh
telnet localhost 2323
curl -d 'username=admin&password=x' http://localhost:8080/login
```

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

## Configuration

Settings live in a TOML file read from `EASYDECOY_CONFIG` (default:
`config/easydecoy.toml`). Every setting has a default, and a misspelled key is
an error. See [config/easydecoy.toml](config/easydecoy.toml) for what each one
does.

## License

MIT
