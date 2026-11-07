---
title: EasyDecoy, decoy services that tell you when someone is inside
hide:
  - navigation
  - toc
---

<div class="es-home">
<header class="es-band es-nav">
<div class="es-wrap es-nav-inner">
<a href="." class="es-brand">
<img src="assets/logo.svg" alt="EasyDecoy" />
<span class="es-brand-name">Easy<span>Decoy</span></span>
</a>
<nav class="es-nav-links" aria-label="Page sections">
<a href="#decoys">Decoys</a>
<a href="#why">Why it works</a>
<a href="#architecture">Architecture</a>
<a href="#install">Install</a>
<a href="install/">Docs</a>
</nav>
<div class="es-nav-actions">
<a class="es-btn es-btn--secondary" href="https://github.com/easysysio/EasyDecoy" target="_blank" rel="noopener noreferrer">
<svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"></path></svg>
GitHub
</a>
<a class="es-btn es-btn--primary" href="#install">Get started</a>
</div>
</div>
</header>
<section class="es-band es-hero">
<div class="es-wrap es-hero-grid">
<div class="es-hero-copy es-rise">
<div class="es-eyebrow"><span class="es-eyebrow-dot"></span>Honeypot · part of EasySYS</div>
<h1 class="es-h1">Nobody should knock. When someone does, you'll know.</h1>
<p class="es-lead">EasyDecoy runs fake network services, a Telnet login and a web admin panel, that look real and lead nowhere. Nothing legitimate ever connects to them, so every connection, request and login attempt is worth a look. Each one is recorded as a JSON event and can be forwarded over syslog.</p>
<div class="es-actions">
<a class="es-btn es-btn--primary es-btn--lg" href="#install">
Get started
<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 5v14M6 13l6 6 6-6"></path></svg>
</a>
<a class="es-btn es-btn--ghost es-btn--lg" href="install/">Read the docs</a>
</div>
<div class="es-pills">
<span class="es-pill"><svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="6" y="6" width="12" height="12" rx="2"></rect><path d="M9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4"></path></svg>Rust core</span>
<span class="es-pill"><svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 8l-9-5-9 5v8l9 5 9-5V8z"></path><path d="M3 8l9 5 9-5M12 13v8"></path></svg>Single binary</span>
<span class="es-pill"><svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="11" width="14" height="10" rx="2"></rect><path d="M8 11V7a4 4 0 018 0v4"></path></svg>Never stores passwords</span>
<span class="es-pill"><svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01"></path></svg>JSON + syslog</span>
</div>
</div>
<div class="es-terminal-wrap es-rise es-rise--late">
<div class="es-terminal">
<div class="es-terminal-accent"></div>
<div class="es-terminal-bar">
<div class="es-dots"><span></span><span></span><span></span></div>
<span class="es-mono">intruder@10.0.4.17 — bash</span>
<span style="width: 47px;"></span>
</div>
<div class="es-term-body es-mono"><span class="t-c"># someone on your network tries the "file server"</span>
<span class="t-p">$</span> telnet fileserver01
Ubuntu 22.04.4 LTS
fileserver01 login: admin
Password:
Login incorrect
<span class="t-c"># EasyDecoy has already written it down</span>
<span class="t-ok">→</span> {"decoy":"telnet","kind":"login_attempt",
   "src_ip":"10.0.4.17","username":"admin",
   "password_length":8}</div>
<div class="es-term-meta">
<div><div class="es-term-meta-label">Decoys</div><div class="es-term-meta-value es-mono">telnet · http</div></div>
<div><div class="es-term-meta-label">Events</div><div class="es-term-meta-value es-mono">JSON · syslog</div></div>
<div><div class="es-term-meta-label">Arch</div><div class="es-term-meta-value es-mono">x86_64 · arm64</div></div>
</div>
</div>
</div>
</div>
<div class="es-strip">
<div class="es-wrap es-strip-inner">
<span class="es-strip-label">Packaged for</span>
<div class="es-strip-items">
<span>Debian / Ubuntu</span>
<span>RHEL / Fedora</span>
<span>openSUSE / SLES</span>
<span>.deb / .rpm</span>
<span class="es-mono">x86_64 · arm64</span>
</div>
</div>
</div>
</section>
<section id="decoys" class="es-band es-section">
<div class="es-wrap">
<div class="es-head">
<div>
<span class="es-kicker-lg">Decoys</span>
<h2 class="es-h2">Two services, built to be found.</h2>
</div>
<p class="es-desc">Each decoy answers like the real thing for long enough to be convincing, then refuses. Nothing a client sends is ever run or passed to the host: the decoys only imitate prompts and replies.</p>
</div>
<div class="ed-types">
<div class="es-card ed-type">
<div class="ed-type-top">
<div class="es-icon"><svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2"></rect><path d="M7 9l3 3-3 3M13 15h4"></path></svg></div>
<span class="es-tag es-mono">:23</span>
</div>
<span class="es-kicker">Telnet</span>
<h3 class="es-h3">A login prompt that never lets anyone in</h3>
<p class="es-product-text">An Ubuntu-style banner and <code>login:</code> prompt that hides the password as it is typed, pauses like a real check, answers <code>Login incorrect</code>, and hangs up after three tries.</p>
<div class="es-tags"><span class="es-tag">connect</span><span class="es-tag">login_attempt</span></div>
</div>
<div class="es-card ed-type">
<div class="ed-type-top">
<div class="es-icon"><svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9"></circle><path d="M3 12h18M12 3a14 14 0 010 18M12 3a14 14 0 000 18"></path></svg></div>
<span class="es-tag es-mono">:80</span>
</div>
<span class="es-kicker">Web admin panel</span>
<h3 class="es-h3">A sign-in page with nothing behind it</h3>
<p class="es-product-text">An appliance-style sign-in page on <code>/</code> and <code>/login</code>, a 404 everywhere else, and every sign-in refused. Each request is recorded with its path, Host and User-Agent.</p>
<div class="es-tags"><span class="es-tag">connect</span><span class="es-tag">request</span><span class="es-tag">login_attempt</span><span class="es-tag">non_http</span></div>
</div>
<div class="es-card ed-type ed-type-wide">
<div class="ed-type-top">
<div class="es-icon"><svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 20h9"></path><path d="M16.5 3.5a2.1 2.1 0 013 3L7 19l-4 1 1-4 12.5-12.5z"></path></svg></div>
<span class="es-tag es-mono">easydecoy.toml</span>
</div>
<span class="es-kicker">Made to look like yours</span>
<h3 class="es-h3">Named the way your real machines are</h3>
<p class="es-product-text">A decoy called <code>fileserver01</code> on a network of <code>fs-ams-02</code>s stands out. Set the banner, the hostname in the prompt, the web server's <code>Server</code> header and the sign-in page's title to match what an intruder expects to find.</p>
<div class="es-tags"><span class="es-tag">banner</span><span class="es-tag">hostname</span><span class="es-tag">server_header</span><span class="es-tag">title</span></div>
</div>
</div>
</div>
</section>
<section id="why" class="es-band es-section es-subtle">
<div class="es-wrap">
<div class="es-head--stack">
<span class="es-kicker-lg">Why it works</span>
<h2 class="es-h2">High signal, by construction.</h2>
<p class="es-desc">Intrusion detection usually means telling the attack apart from a flood of normal traffic. A decoy has no normal traffic, so whatever reaches it is the signal.</p>
</div>
<div class="ed-features">
<div class="es-card ed-feature">
<div class="es-icon es-icon--sm"><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 8a6 6 0 0112 0c0 7 3 9 3 9H3s3-2 3-9"></path><path d="M10.3 21a1.94 1.94 0 003.4 0"></path></svg></div>
<h3 class="ed-feature-title">Alerts worth reading</h3>
<p class="ed-feature-text">Nobody has a reason to log in to a server that doesn't exist. A scan, a guessed password or a curious script is exactly what you wanted to hear about.</p>
</div>
<div class="es-card ed-feature">
<div class="es-icon es-icon--sm"><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="11" width="14" height="10" rx="2"></rect><path d="M8 11V7a4 4 0 018 0v4"></path></svg></div>
<h3 class="ed-feature-title">Passwords never stored</h3>
<p class="ed-feature-text">A login attempt keeps the username and the password's length. A colleague who types their real password into the wrong window doesn't end up in a log file.</p>
</div>
<div class="es-card ed-feature">
<div class="es-icon es-icon--sm"><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3l8 3v6c0 5-3.5 8.5-8 9-4.5-.5-8-4-8-9V6l8-3z"></path><path d="M9 12l2 2 4-4"></path></svg></div>
<h3 class="ed-feature-title">Nothing is executed</h3>
<p class="ed-feature-text">The decoys are low-interaction: scripted prompts and replies, no shell and no real service behind them. There is nothing for an intruder to break out of.</p>
</div>
<div class="es-card ed-feature">
<div class="es-icon es-icon--sm"><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 14l4-4"></path><path d="M3.34 19a10 10 0 1117.32 0"></path></svg></div>
<h3 class="ed-feature-title">Bounded under attack</h3>
<p class="ed-feature-text">Sessions are capped in number, idle time and length, and every read has a size limit. A flood of connections costs sockets for a moment, never memory.</p>
</div>
<div class="es-card ed-feature">
<div class="es-icon es-icon--sm"><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="8" r="4"></circle><path d="M4 21v-1a6 6 0 0112 0v1"></path><path d="M19 8v6M16 11h6"></path></svg></div>
<h3 class="ed-feature-title">Least privilege</h3>
<p class="ed-feature-text">The service runs as its own account with one capability, binding ports below 1024, inside a systemd sandbox that leaves it the log directory and nothing else to write to.</p>
</div>
<div class="es-card ed-feature">
<div class="es-icon es-icon--sm"><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01"></path></svg></div>
<h3 class="ed-feature-title">JSON lines and syslog</h3>
<p class="ed-feature-text">One JSON object per event, ready for <code>jq</code>, and a copy to any syslog collector, EasyLog included. A collector that is down never holds up the local record.</p>
</div>
</div>
</div>
</section>
<section id="architecture" class="es-band es-section">
<div class="es-wrap">
<div class="es-head--stack">
<span class="es-kicker-lg">Architecture</span>
<h2 class="es-h2">Decoys on your network, events on your desk.</h2>
<p class="es-desc">Each decoy reports what happens to it to a single event writer, which appends one JSON line to the events file and hands a copy to the syslog sender. You read the file directly, or the collector it feeds.</p>
</div>
<div class="es-diagram">
<div class="es-diagram-scroll">
<svg viewBox="0 0 1120 440" role="img" aria-label="Diagram: port scans, intruders and malware reach the Telnet and HTTP decoys inside EasyDecoy; both report to an event writer, which appends to events.jsonl, read by administrators, and sends a copy over syslog to EasyLog or another collector.">
<defs>
<marker id="ed-ah" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path class="a-head" d="M0 0L10 5L0 10z"></path></marker>
<marker id="ed-ahb" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path class="a-head a-head--es" d="M0 0L10 5L0 10z"></path></marker>
</defs>
<text class="a-lane" x="0" y="14">YOUR NETWORK</text>
<text class="a-lane" x="300" y="14">EASYDECOY</text>
<text class="a-lane" x="900" y="14">YOU</text>
<g class="a-ext"><rect x="0" y="46" width="220" height="76" rx="10"></rect><text class="a-title" x="20" y="78">Port scans</text><text class="a-sub" x="20" y="102">nmap · masscan</text></g>
<g class="a-ext"><rect x="0" y="182" width="220" height="76" rx="10"></rect><text class="a-title" x="20" y="214">Intruders</text><text class="a-sub" x="20" y="238">moving between hosts</text></g>
<g class="a-ext"><rect x="0" y="318" width="220" height="88" rx="10"></rect><text class="a-title" x="20" y="354">Malware</text><text class="a-sub" x="20" y="380">worms · botnets</text></g>
<g class="a-es"><rect x="300" y="30" width="560" height="386" rx="14"></rect><text class="a-title" x="324" y="66">EasyDecoy</text><text class="a-sub" x="324" y="90">one binary · own account · sandboxed</text></g>
<g class="a-box"><rect x="324" y="132" width="150" height="80" rx="10"></rect><text class="a-title" x="340" y="164">Telnet decoy</text><text class="a-sub" x="340" y="188">:23 login</text></g>
<g class="a-box"><rect x="324" y="292" width="150" height="92" rx="10"></rect><text class="a-title" x="340" y="328">HTTP decoy</text><text class="a-sub" x="340" y="354">:80 sign-in</text></g>
<g class="a-box"><rect x="505" y="212" width="150" height="80" rx="10"></rect><text class="a-title" x="521" y="244">Event writer</text><text class="a-sub" x="521" y="268">one JSON line</text></g>
<g class="a-box"><rect x="686" y="132" width="150" height="80" rx="10"></rect><text class="a-title" x="702" y="164">events.jsonl</text><text class="a-sub" x="702" y="188">/var/log/…</text></g>
<g class="a-box"><rect x="686" y="292" width="150" height="92" rx="10"></rect><text class="a-title" x="702" y="328">Syslog</text><text class="a-sub" x="702" y="354">UDP · TCP</text></g>
<g class="a-ext"><rect x="900" y="134" width="220" height="76" rx="10"></rect><text class="a-title" x="920" y="166">Administrators</text><text class="a-sub" x="920" y="190">tail · jq</text></g>
<g class="a-es"><rect x="900" y="294" width="220" height="88" rx="10"></rect><text class="a-title" x="922" y="330">EasyLog</text><text class="a-sub" x="922" y="356">or any collector</text></g>
<path class="a-line" fill="none" d="M220 84 H262 V160 H322" marker-end="url(#ed-ah)"></path>
<text class="a-label" x="226" y="76">telnet</text>
<path class="a-line" fill="none" d="M220 220 H276 V326 H322" marker-end="url(#ed-ah)"></path>
<text class="a-label" x="226" y="212">http</text>
<path class="a-line" fill="none" d="M220 362 H262 V350 H322" marker-end="url(#ed-ah)"></path>
<text class="a-label" x="226" y="354">http</text>
<path class="a-line a-line--es" fill="none" d="M474 172 H490 V236 H503" marker-end="url(#ed-ahb)"></path>
<path class="a-line a-line--es" fill="none" d="M474 338 H490 V268 H503" marker-end="url(#ed-ahb)"></path>
<path class="a-line a-line--es" fill="none" d="M655 240 H670 V172 H684" marker-end="url(#ed-ahb)"></path>
<path class="a-line a-line--es" fill="none" d="M655 264 H670 V338 H684" marker-end="url(#ed-ahb)"></path>
<line class="a-line" x1="836" y1="172" x2="898" y2="172" marker-end="url(#ed-ah)"></line>
<text class="a-label" x="867" y="162" text-anchor="middle">read</text>
<line class="a-line a-line--es" x1="836" y1="338" x2="898" y2="338" marker-end="url(#ed-ahb)"></line>
<text class="a-label a-label--es" x="867" y="328" text-anchor="middle">copy</text>
</svg>
</div>
<div class="es-legend">
<span><span class="es-swatch"></span>EasySYS service</span>
<span><span class="es-swatch es-swatch--ext"></span>Your network and people</span>
<span class="es-legend-note">The events file is always written first. Syslog is a copy, and a collector that is down never holds it up.</span>
</div>
</div>
</div>
</section>
<section id="install" class="es-band es-section es-subtle">
<div class="es-wrap es-install">
<div>
<span class="es-kicker-lg">Deploy</span>
<h2 class="es-h2">One package. Live when you say so.</h2>
<p class="es-desc">EasyDecoy is published as <code>.deb</code> and <code>.rpm</code> packages for x86_64 and arm64 on every GitHub release. Installing starts nothing: the decoys go live when you enable the service. The <a href="install/">installation guide</a> has the details.</p>
<div class="es-steps">
<div class="es-step"><span class="es-step-num es-mono">1</span><div><div class="es-step-title">Install the package</div><div class="es-step-text">It creates the <code>easydecoy</code> account and a default config.</div></div></div>
<div class="es-step"><span class="es-step-num es-mono">2</span><div><div class="es-step-title">Make the decoys look like yours</div><div class="es-step-text">Ports, banner, hostname and page title live in one TOML file.</div></div></div>
<div class="es-step"><span class="es-step-num es-mono">3</span><div><div class="es-step-title">Enable it and watch</div><div class="es-step-text">Events land in <code>/var/log/easydecoy</code>, and on your syslog collector if you set one.</div></div></div>
</div>
</div>
<div class="es-terminal es-code">
<input class="es-os-radio" type="radio" name="es-os" id="es-os-deb" checked />
<input class="es-os-radio" type="radio" name="es-os" id="es-os-rpm" />
<input class="es-os-radio" type="radio" name="es-os" id="es-os-suse" />
<input class="es-os-radio" type="radio" name="es-os" id="es-os-air" />
<div class="es-tabs">
<label for="es-os-deb">Debian / Ubuntu</label>
<label for="es-os-rpm">RHEL / Fedora</label>
<label for="es-os-suse">openSUSE / SLES</label>
<label for="es-os-air">From source</label>
</div>
<div class="es-panel es-panel--deb"><div class="es-term-body es-mono"><span class="t-c"># 1 — download from github.com/easysysio/EasyDecoy/releases</span>
<span class="t-p">$</span> sudo apt install ./easydecoy_*_amd64.deb   <span class="t-c"># or _arm64.deb</span>
<span class="t-c"># 2 — make the decoys look like yours</span>
<span class="t-p">$</span> sudoedit /etc/easydecoy/easydecoy.toml
<span class="t-c"># 3 — go live, and watch</span>
<span class="t-p">$</span> sudo systemctl enable --now easydecoy
<span class="t-p">$</span> sudo tail -f /var/log/easydecoy/events.jsonl</div></div>
<div class="es-panel es-panel--rpm"><div class="es-term-body es-mono"><span class="t-c"># 1 — download from github.com/easysysio/EasyDecoy/releases</span>
<span class="t-p">$</span> sudo dnf install ./easydecoy-*.x86_64.rpm   <span class="t-c"># or .aarch64.rpm</span>
<span class="t-c"># 2 — make the decoys look like yours</span>
<span class="t-p">$</span> sudoedit /etc/easydecoy/easydecoy.toml
<span class="t-c"># 3 — go live, and watch</span>
<span class="t-p">$</span> sudo systemctl enable --now easydecoy
<span class="t-p">$</span> sudo tail -f /var/log/easydecoy/events.jsonl</div></div>
<div class="es-panel es-panel--suse"><div class="es-term-body es-mono"><span class="t-c"># 1 — download from github.com/easysysio/EasyDecoy/releases</span>
<span class="t-p">$</span> sudo zypper install ./easydecoy-*.x86_64.rpm   <span class="t-c"># or .aarch64.rpm</span>
<span class="t-c"># 2 — make the decoys look like yours</span>
<span class="t-p">$</span> sudoedit /etc/easydecoy/easydecoy.toml
<span class="t-c"># 3 — go live, and watch</span>
<span class="t-p">$</span> sudo systemctl enable --now easydecoy
<span class="t-p">$</span> sudo tail -f /var/log/easydecoy/events.jsonl</div></div>
<div class="es-panel es-panel--air"><div class="es-term-body es-mono"><span class="t-c"># 1 — build it (needs a Rust toolchain)</span>
<span class="t-p">$</span> git clone https://github.com/easysysio/EasyDecoy
<span class="t-p">$</span> cd EasyDecoy
<span class="t-c"># 2 — run it: telnet on :2323, http on :8080</span>
<span class="t-p">$</span> cargo run | jq .
<span class="t-c"># 3 — knock, from another terminal</span>
<span class="t-p">$</span> telnet localhost 2323</div></div>
</div>
</div>
</section>
<section class="es-band es-cta">
<div class="es-wrap">
<div class="es-cta-box">
<svg class="es-cta-hex" viewBox="0 0 512 512" aria-hidden="true"><polygon points="86,256 171,109 341,109 426,256 341,403 171,403" fill="none" stroke="#ffffff" stroke-width="34" stroke-linejoin="round"></polygon></svg>
<div class="es-cta-copy">
<h2 class="es-h2">Put one decoy on an unused address.</h2>
<p class="es-cta-text">Give it a name that fits in, and wait. If your network is quiet, so is EasyDecoy. MIT licensed and developed in the open.</p>
</div>
<div class="es-cta-actions">
<a class="es-btn es-btn--primary es-btn--lg" href="install/">Read the docs</a>
<a class="es-btn es-btn--on-dark es-btn--lg" href="https://github.com/easysysio/EasyDecoy" target="_blank" rel="noopener noreferrer">GitHub</a>
</div>
</div>
</div>
</section>
<footer class="es-band es-footer">
<div class="es-wrap es-footer-grid">
<div>
<a href="." class="es-brand"><img src="assets/logo.svg" alt="EasyDecoy" /><span class="es-brand-name">Easy<span>Decoy</span></span></a>
<p class="ed-small">Decoy services that tell you when someone is inside. Part of the <a href="https://easysys.io">EasySYS</a> suite.</p>
</div>
<div class="es-footer-col">
<span class="es-footer-title">Documentation</span>
<a href="install/">Installation</a>
<a href="configuration/">Configuration</a>
<a href="decoys/">Decoys</a>
<a href="events/">Events</a>
<a href="syslog/">Syslog</a>
<a href="security/">Security model</a>
</div>
<div class="es-footer-col">
<span class="es-footer-title">EasySYS</span>
<a href="https://easysys.io">easysys.io</a>
<a href="https://easylog.easysys.io">EasyLog</a>
<a href="https://easywaf.easysys.io">EasyWAF</a>
<a href="https://easyvault.easysys.io">EasyVault</a>
<a href="https://easydc.easysys.io">EasyDC</a>
<a href="https://easynas.org">EasyNAS</a>
</div>
<div class="es-footer-col">
<span class="es-footer-title">Community</span>
<a href="https://github.com/easysysio/EasyDecoy">GitHub</a>
<a href="https://github.com/easysysio/EasyDecoy/releases">Releases</a>
<a href="https://discord.gg/easysys">Discord</a>
</div>
</div>
<div class="es-wrap">
<div class="es-footer-bottom">
<span>© 2026 EasySYS · MIT licensed</span>
<span class="es-mono">easydecoy.easysys.io</span>
</div>
</div>
</footer>
</div>
