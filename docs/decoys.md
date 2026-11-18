# Decoys

A decoy is a fake network service. It answers like the real thing for long
enough to be convincing, records what happens, and refuses. EasyDecoy's decoys
are **low-interaction**: they imitate prompts and replies, and nothing a client
sends is ever run or passed to the host.

Every decoy records a `connect` event the moment a client connects, before
anything else, so a port scan that connects and leaves still shows up. What
each decoy records after that is below; [Events](events.md) has the fields.

## Telnet

A Linux host's login prompt:

```text
Ubuntu 22.04.4 LTS
fileserver01 login: admin
Password:
Login incorrect
fileserver01 login:
```

- The banner comes first, then `<hostname> login:`.
- At the password prompt, EasyDecoy tells the client it will echo. The client
  then stops showing what is typed, as it would for a real password prompt.
- Every login fails after a **2-second** pause. A real check takes a moment,
  and an instant answer gives a decoy away.
- After **three** tries the connection is closed.

Each try is recorded as a `login_attempt` with the `username` and the
`password_length`, never the password itself.

Telnet clients mix option negotiation into what the user types. EasyDecoy
strips it as bytes arrive, so what is recorded is what was typed, even when a
negotiation is split across two packets. A line longer than 1024 bytes is cut
off rather than buffered.

## Web admin panel

A small appliance's web interface:

| Request | Answer | Recorded as |
|---|---|---|
| `GET /` or `GET /login` | The sign-in page | `request` |
| `POST /login` | The sign-in page with *Invalid username or password*, after a 1-second pause | `login_attempt` |
| Any other path | A plain 404 page | `request` |
| `HEAD` | The headers only | `request` |
| Something that isn't HTTP | Nothing; the connection is closed | `non_http` |

- Every request records its `method`, `path`, `host` and `user_agent`.
- A sign-in also records the `username` and the `password_length`. The form
  fields looked for are `username`, `user`, `login` or `email`, and
  `password`, `pass` or `pwd`, so a script written for another product's form
  is still caught.
- Something that isn't HTTP, such as an SSH client or a protocol probe, is
  recorded as `non_http` with a `sample`: the first 64 bytes it sent. A client
  that sends a few bytes and waits is recorded when it gives up, goes quiet
  for the idle timeout, or has sent 8 KiB without a complete request.

The decoy handles one request per connection and closes it (`Connection:
close`). The request head may be at most **8 KiB** and the body at most
**4 KiB**. Anything longer is cut off, not buffered.

## FTP

A file server that never lets anyone in, answering the way vsftpd does with
anonymous logins turned off:

```text
220 (vsFTPd 3.0.5)
USER admin
331 Please specify the password.
PASS ********
530 Login incorrect.
```

- The greeting is `220` followed by the **banner**.
- Every login fails after a **1-second** pause, `anonymous` included. After
  **three** failed logins the connection is closed, as vsftpd does.
- Each try is recorded as a `login_attempt` with the `username` and the
  `password_length`, never the password. An anonymous login's "password" is
  usually an email address, so that isn't kept either.
- Nobody gets past the login, so there is no data connection and no file to
  read. Anything that would need a login gets
  `530 Please login with USER and PASS.`

What a client sends on its way to a login (`FEAT`, `SYST`, `AUTH TLS`, `OPTS`,
`NOOP`, `HELP`, `QUIT` and the like) is answered and not recorded. Any other
command, such as `SITE EXEC`, `RETR` or `PORT` sent without a login, is
recorded as a `command` event with the `command` and its `argument`: an
ordinary client has no reason to send it. A connection is closed after 50
commands, and a line longer than 1024 bytes is cut off rather than buffered.

The packaged service starts it on port **21**. An upgrade from 0.1.0, which
had no FTP decoy, leaves it off; turn it on from the **Decoys** page.

## SSH

An SSH server that answers like a stock Ubuntu one and lets nobody in:

```text
$ ssh root@fileserver01
root@fileserver01's password:
Permission denied, please try again.
root@fileserver01's password:
Permission denied, please try again.
root@fileserver01's password:
root@fileserver01: Permission denied (publickey,password).
```

SSH is encrypted after its first line, so this decoy really does the key
exchange, with a host key of its own, before it refuses anything.

- It introduces itself as `SSH-2.0-` followed by the **version**,
  `OpenSSH_8.9p1 Ubuntu-3ubuntu0.10` by default. That line is the first thing
  any client or scanner reads, so make it match your real servers.
- It offers what Ubuntu's sshd offers: **public key** and **password**.
- A wrong password is refused after **2 seconds**, as PAM does on a real host.
  A key is refused at once. After **six** refusals the client is
  disconnected, as OpenSSH's `MaxAuthTries` does.
- Nobody gets past the login, so there is never a shell, a command,
  a file transfer or a forwarded port.

What it records:

- A `handshake` as soon as the client says what it is, such as
  `SSH-2.0-OpenSSH_9.6` or `SSH-2.0-Go`, so a scanner that reads the banner
  and leaves is seen too. Something that isn't SSH is recorded the same way:
  its first line.
- A `login_attempt` for each password, with the `username` and the
  `password_length`, never the password.
- A `login_attempt` for each public key offered, with the `username`, the
  key's type and its SHA-256 fingerprint, as `ssh-keygen -lf` shows it. A
  fingerprint seen on several decoys, or again next month, is the same key.

The host key is an Ed25519 key that EasyDecoy makes on its first start and
keeps in the database, so a client that comes back sees the same fingerprint.
The **Decoys** page shows it. It is the decoy's own key, never the host's.

### Port 22

The packaged service sets the SSH decoy up on port **22** but leaves it
**off**: on almost every server, 22 is the host's own sshd, the one you use to
manage it. To give 22 to the decoy:

1. Move the real sshd to another port, say 2222. In
   `/etc/ssh/sshd_config`, set `Port 2222`, then restart it
   (`sudo systemctl restart ssh`, or `sshd` on RHEL and SUSE). Allow the new
   port in the host's firewall and in SELinux
   (`sudo semanage port -a -t ssh_port_t -p tcp 2222`) first if they are on.
2. **From a second terminal, check that `ssh -p 2222 decoy-host` works**
   before you close the session you are in.
3. Turn the SSH decoy on in the GUI. Its card says **Running on 0.0.0.0:22**.

From then on, reach the host with `-p 2222`, and the GUI's tunnel too:

```bash
ssh -p 2222 -L 9443:127.0.0.1:9443 decoy-host
```

If sshd still has 22, the decoy can't take it: its card says **Not running**,
and nothing else changes. The packaged service also starts after sshd, so on
a reboot sshd gets 22 first, never the decoy.

!!! warning "Your own `ssh` will knock on it"
    Once the decoy has port 22, `ssh decoy-host` without `-p 2222` reaches the
    decoy. Your client warns that the host key has changed, and if you go on,
    your login attempt is recorded like anyone's: the username and the
    password's length, never the password. Add `Port 2222` for the host to
    your `~/.ssh/config`.

## Making them believable

A decoy only works if an intruder believes it.

- **Name it like your real machines.** If your file servers are called
  `fs-ams-02`, a `fileserver01` stands out. On the GUI's **Decoys** page, set
  the hostname, banners, Server header and page title to fit in. A file
  server whose FTP greeting says ProFTPD where your others run vsftpd is a
  tell, and so is an SSH version from another distribution.
- **Give it an address nothing uses.** No legitimate client should have any
  reason to reach it, so every event means something.
- **Keep it out of anything that would be used normally.** Don't put the
  decoy in DNS names that people or scripts resolve every day, or in
  monitoring that polls it, or the events fill with your own traffic.
- **Use the real ports.** An intruder scanning for Telnet tries 23, not 2323.
  The package does this for you.

## Limits

These apply to every decoy together:

| Limit | Default | Setting |
|---|---|---|
| Open sessions | 256 | `max_connections` |
| Silence before a client is dropped | 30 s | `idle_timeout_secs` |
| Longest session | 120 s | `session_limit_secs` |

At the session limit, new connections are closed at once. A flood of
connections costs a socket for a moment, never memory or a stuck decoy.
