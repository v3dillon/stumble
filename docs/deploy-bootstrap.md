# Deploy a Bootstrap/Index/Relay node on a VPS

A Bootstrap node gives the network an initial meeting point: Origins push signed
Pod Announcements to it, and Home Nodes pull its topic-neutral Announcement
Stream. An Index node answers explicit Explore queries over the same admitted
announcements. A Relay node caches and serves Origin-signed Pod snapshots so a
private Home Node can publish without a public listener. No role gains
authority — every artifact stays Origin-signed and is re-verified by each Home
Node (ADR-0037, ADR-0046, ADR-0055).

Any small VPS works: 1 vCPU / 1 GB RAM is plenty. The store is a single SQLite
file.

## Quick deploy (Ubuntu/Debian)

1. Create a DNS A (and AAAA) record for your domain, e.g.
   `bootstrap.example.com`, pointing at the VPS.
2. On the VPS:

```bash
git clone https://github.com/<you>/stumble && cd stumble
sudo ./scripts/deploy-bootstrap-vps.sh bootstrap.example.com
```

Pass `--no-index` and/or `--no-relay` after the domain to turn off those roles.
The three capabilities stay independent flags on one process, never one fused
role.

## Coolify (same VPS, Coolify owns TLS)

Do **not** run `deploy-bootstrap-vps.sh` on a Coolify host. That script
installs Caddy and overwrites `/etc/caddy/Caddyfile`. Use the root
`Dockerfile` instead and let Coolify terminate HTTPS.

1. In Cloudflare, create an A (and AAAA) record for
   `bootstrap.stumble.network` that points at the VPS. Keep the record
   DNS-only (grey cloud) so Coolify can issue the certificate.
2. In Coolify: **New Resource** → **Git Repository** → `v3dillon/stumble`,
   branch `main`.
3. Build pack: **Dockerfile** (root `Dockerfile`). Port: `8787`.
   Health check path: `/health`.
4. Persistent storage: one volume mounted at `/data`. One replica only —
   SQLite does not accept two writers.
5. Environment:

```bash
STUMBLE_DATA_DIR=/data/node
STUMBLE_CREDENTIAL_STORE_DIR=/data/credentials
STUMBLE_BASE_URL=https://bootstrap.stumble.network
```

   Leave `STUMBLE_BOOTSTRAP`, `STUMBLE_INDEX`, and `STUMBLE_RELAY` unset
   (they default on). Set any of them to `0` to disable that role.
6. Domains: `https://bootstrap.stumble.network`.
7. Deploy. First start runs `stumble node init` into `/data`. Later
   deploys reuse that volume.

The script is idempotent — to upgrade, `git pull` and re-run it. Note that it
owns the entire `/etc/caddy/Caddyfile`: every run overwrites the file with its
single domain block, clobbering any other site served from the box. It:

- installs Rust and [Caddy](https://caddyserver.com) if missing, and builds
  release binaries into `/usr/local/bin`;
- creates a `stumble` system user with a Home Node under `/var/lib/stumble`
  (file-based Owner Credential store via `STUMBLE_CREDENTIAL_STORE_DIR`);
- installs a hardened `stumble-bootstrap` systemd unit running
  `stumble-api --bootstrap --index --relay --bind 127.0.0.1:8787 --base-url https://<domain>`;
- configures Caddy to terminate TLS for the domain (certificates are automatic
  via Let's Encrypt) and reverse-proxy to the loopback bind.

## Verify

```bash
curl https://bootstrap.example.com/.well-known/stumble-node
curl "https://bootstrap.example.com/bootstrap/announcements/stream?limit=5"
```

The well-known response advertises `bootstrap_announcements`,
`bootstrap_announcement_stream`, (with the Index role)
`index_search_announcements`, and (with the Relay role) `relay_publications`,
`relay_pod_snapshot_template`, and `relay_explore_samples_template`. A process
with a role off never advertises that role.

## Onboard friends with one link

The node serves the onboarding script with its own URL pre-filled:

```
https://bootstrap.example.com/llms.txt
```

A bare pasted URL usually gets you a fetch-and-summary, not action — and
good harnesses are rightly wary of executing instructions fetched from the
web without a human ask. So send it with one explicit line, e.g.:

> Paste this to your AI and tell it: "follow the onboarding steps in
> https://bootstrap.example.com/llms.txt"

With that human instruction attached, the harness fetches the script and
walks your friend through install, taste, and their first discovery run
against your Bootstrap.

## Point nodes at it

Home Nodes (readers):

```bash
stumble sync bootstrap add --label my-bootstrap --base-url https://bootstrap.example.com
stumble sync discovery index add --label my-bootstrap --base-url https://bootstrap.example.com
```

The runner daemon then pulls the Announcement Stream on its network-sync
interval; `stumble pod explore` fans explicit queries out to the Index.

Origins (publishers) need nothing extra: `stumble pod publish` and the daemon's
re-announce tick push announcements to every enabled Bootstrap endpoint
automatically.

Private Origins (no public listener) publish through the Relay:

```bash
stumble pod publish <slug> --base-url https://bootstrap.example.com --via-relay
stumble pod relay-push <slug> --relay-url https://bootstrap.example.com  # later updates
```

The share URL becomes
`https://bootstrap.example.com/relay/pods/<origin-node-id>/<slug>`. Friends
subscribe to that URL; their nodes pin the Origin identity from the signed
snapshot, never the Relay. The Relay stores only the signed public snapshot —
no Taste Profile, Subscriptions, Candidates, or Owner credentials.

## Operate

```bash
systemctl status stumble-bootstrap    # service health
journalctl -u stumble-bootstrap -f    # logs
```

Admission is open by design — no accounts — and protected by signature
verification, live Origin reachability probes, per-origin and network-wide
rate limits, lease expiry, and payload bounds. Do not add login or client
identifiers to these paths (see `docs/sponsored-bootstrap-operators.md` for
the operator ground rules).

## Uninstall

```bash
sudo systemctl disable --now stumble-bootstrap
sudo rm /etc/systemd/system/stumble-bootstrap.service /usr/local/bin/stumble /usr/local/bin/stumble-api
sudo rm -r /var/lib/stumble
```

The script wrote `/etc/caddy/Caddyfile` wholesale, so the domain block is all
it contains: replace the file with your own Caddy config (or remove it and
uninstall Caddy) and reload Caddy.
