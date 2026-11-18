# The GUI

EasyDecoy is managed from a web interface served by the binary itself, over
HTTPS, at `https://127.0.0.1:9443/` unless the
[config file](configuration.md#the-config-file) says otherwise. From another
machine, tunnel to it:

```bash
ssh -L 9443:127.0.0.1:9443 decoy-host
```

and open **https://localhost:9443/**. If you gave port 22 to the
[SSH decoy](decoys.md#port-22), add the real sshd's port, as `-p 2222`.

## First visit

While there are no accounts, every page leads to **setup**, which creates the
administrator: a username and a password of at least 10 characters. Setup
signs you in, and from then on the page is gone. Nobody else can create the
first account by getting there after you.

## The pages

**Dashboard**
:   What the decoys have seen over the last 24 hours, 7 days or 30 days: the
    number of events, login attempts and source addresses, a timeline, the
    sources that knock most, the usernames tried most, the HTTP paths asked
    for most, and the latest events. Each decoy's state is shown too. A
    source, username or path links to its events.

**Events**
:   Every event, newest first, 50 to a page. Filter by decoy, kind, source
    address, or text anywhere in the event, such as a username or a path.
    A row opens to show every field the event recorded. **Export** downloads
    the events the filter selects as [JSON lines](events.md), exactly as
    they are stored, for `jq` or another tool.

**Decoys**
:   Each decoy's state and settings: Telnet, HTTP, FTP and SSH, with the SSH
    decoy's host key fingerprint. A decoy that
    couldn't start, usually because its port is in use, says **Not running**
    and why. EasyDecoy tries
    it again every 30 seconds, or as soon as you save it.

**Settings**
:   The limits the decoys run under, how long events are kept, an optional
    events file, and [syslog](syslog.md). The
    [configuration page](configuration.md#settings-in-the-gui) lists every
    setting and its default.

**Account**
:   Your password, and the **activity** log: every sign-in, failed sign-in,
    sign-out, export and change made in the GUI, with who made it, from where,
    and what the settings became.

Times are shown in your browser's time zone. Hover over one for the exact
UTC time it was stored with.

## Sign-in and sessions

- Passwords are stored as bcrypt hashes.
- A session lasts **12 hours**, or until the browser closes or you sign out.
  The cookie holds a random token, and the database keeps only its SHA-256, so
  a copy of the database can't be used to sign in.
- Changing your password signs out every other browser signed in as you.
- After **5 failed sign-ins from one address in 15 minutes**, that address is
  refused until the oldest ages off. Every failure takes a second to answer.
- Every form carries a token tied to the session, and a form sent from another
  site is refused, so a page you visit elsewhere can't change settings.
- Pages forbid framing, aren't cached, and load nothing from outside the GUI.

To get back in after losing the password, stop the service, delete the
accounts and start it again. The next visit is setup:

```bash
sudo systemctl stop easydecoy
sudo sqlite3 /var/lib/easydecoy/easydecoy.db 'DELETE FROM users'
sudo systemctl start easydecoy
```

Settings and events are untouched.

## Backing up

Everything EasyDecoy keeps is in the database. Stopping the service writes all
of it into the `.db` file itself, so a copy of that one file is a complete
backup:

```bash
sudo systemctl stop easydecoy
sudo cp /var/lib/easydecoy/easydecoy.db /root/easydecoy-backup.db
sudo systemctl start easydecoy
```

While it runs, `sqlite3 /var/lib/easydecoy/easydecoy.db ".backup /root/easydecoy-backup.db"`
makes a consistent copy without stopping it.
