# Installation

EasyDecoy ships as a single binary and installs a **service** that runs the
decoys and the management GUI. Every
[GitHub release](https://github.com/easysysio/EasyDecoy/releases) has `.deb`
and `.rpm` packages for **x86_64** and **arm64**.

Installing starts nothing. The decoys go live when you enable the service.

=== "Debian / Ubuntu"

    ```bash
    sudo apt install ./easydecoy_<version>-1_amd64.deb     # or _arm64.deb
    ```

=== "RHEL / Fedora"

    ```bash
    sudo dnf install ./easydecoy-<version>-1.x86_64.rpm    # or .aarch64.rpm
    ```

=== "openSUSE / SLES"

    ```bash
    sudo zypper install ./easydecoy-<version>-1.x86_64.rpm # or .aarch64.rpm
    ```

Then start it:

```bash
sudo systemctl enable --now easydecoy
```

## Open the GUI

The GUI listens on **this machine only**, at `https://127.0.0.1:9443/`. A decoy
host's other addresses are there for intruders to find, and the GUI must not
be one of the things they find. Reach it from your computer through an SSH
tunnel:

```bash
ssh -L 9443:127.0.0.1:9443 decoy-host
```

Then open **https://localhost:9443/**:

1. Your browser warns about the certificate once. EasyDecoy made it on its
   first start, self-signed. Accept it, or [use your own](configuration.md#the-guis-certificate).
2. Create the administrator account. This page appears only while there are
   no accounts.
3. On **Decoys**, make them look like the rest of your network: the banner,
   the hostname in the prompt, the web server's name and the sign-in page's
   title. Each one restarts as you save it.

The [GUI page](gui.md) walks through the rest.

!!! note "The packaged service uses the real ports"
    On its first start the service puts the Telnet decoy on **23**, the web
    decoy on **80** and the FTP decoy on **21**. If another service on the host already uses one of
    them, that decoy shows **Not running** in the GUI, with the reason, and
    EasyDecoy tries it again every 30 seconds. Move it to another port on the
    Decoys page, or turn it off.

## What the package installs

| Path | What it is |
|---|---|
| `/usr/bin/easydecoy` | The binary. The GUI's pages are built in. |
| `/etc/easydecoy/easydecoy.toml` | The config: the log level, and where the database and the GUI are. Written only if there is none. |
| `/var/lib/easydecoy/easydecoy.db` | The database: settings, events, accounts, the audit trail. Made on the first start. |
| `/usr/lib/systemd/system/easydecoy.service` | The systemd unit |
| `/var/log/easydecoy/` | For an [events file](events.md#an-events-file), if you set one |
| `/etc/logrotate.d/easydecoy` | Weekly rotation of `/var/log/easydecoy/events.jsonl`, twelve weeks kept |

The package also creates a system account, `easydecoy`, with no login shell.
The service runs as that account, never as root. The only extra permission it
gets is binding ports below 1024. The [security model](security.md) explains
the rest of its sandbox.

## Upgrades and removal

- **An upgrade** keeps your config and database as they are, and restarts the
  service if it was running. A service you stopped stays stopped.
- **Removing** the package stops and disables the service, so no decoy is left
  listening. `/etc/easydecoy`, `/var/lib/easydecoy` and `/var/log/easydecoy`
  stay behind.

### From 0.1.0

0.1.0 kept every setting in `/etc/easydecoy/easydecoy.toml`. The upgrade keeps
that file, and on the first start of 0.2.0 its decoy, limit, syslog and
events-file settings become the database's. Nothing changes for the decoys: they
listen where they did, under the same names, and events still go to
`/var/log/easydecoy/events.jsonl`. The FTP decoy, new in 0.2.0, starts off;
turn it on from the **Decoys** page.

From then on the settings are changed in the GUI, and the old ones in the file
are ignored. The log says so at each start until you delete them, leaving
`log_level` and the [0.2.0 keys](configuration.md#the-config-file).

## From source

With a Rust toolchain, EasyDecoy runs straight from a checkout. It reads
`config/easydecoy.toml`, keeps its database in `easydecoy.db` beside it, and
puts the decoys on unprivileged ports (Telnet on 2323, web on 8080, FTP on
2121):

```bash
git clone https://github.com/easysysio/EasyDecoy
cd EasyDecoy
cargo run
```

Open **https://127.0.0.1:9443/** and create the account. Then, from another
terminal, knock:

```bash
telnet localhost 2323
curl -d 'username=admin&password=x' http://localhost:8080/login
```

The events are on the GUI's dashboard. EasyDecoy's own log goes to stderr.

Next: [the GUI](gui.md).
