# Public server: "Play in your browser" on the website

The website (`website/`, a static page on Vercel) can't run the game by itself. The rules engine is Forge, in
Java, and every game against the AI runs on the server. This directory builds that server. It is the normal
engine in `--public` mode, serving the browser client. The store page's big screenshot turns into the game
as soon as the server answers.

What `--public` changes (details in `engine/docs/protocol.md`, "Public engine"):

- Only games against the Forge AI. There are no lobbies, drafts or sealed events, and nothing a visitor does is
  written to disk. Visitors play the bundled decks and don't see the deck builder, Multiplayer or Cubes.
- At most `MTG_DUELSTONE_MAX_MATCHES` games run at once (default 6). When it is full, the website says so
  before anyone downloads the client.
- Each game is bound to the seat token its visitor received. A closed tab concedes after 2 minutes, and the
  finished game is dropped.

## Build

```bash
packaging/server/build.sh              # engine jar + release web client → target/server-bundle → docker image
packaging/server/build.sh --stage-only # just the bundle (no Docker needed to run it, see the script header)
```

## Run

Card images are not in the image. Fill an `offline/img` once with `tools/fetch-offline.sh` (about 9 GB) and
mount it. Without images, cards are drawn as text frames.

```bash
docker run -d --name duelstone --restart unless-stopped \
  -p 127.0.0.1:8080:8080 \
  -v /srv/duelstone/img:/opt/mtg-duelstone/offline/img:ro \
  -v duelstone-data:/data \
  --memory 3g -e MTG_DUELSTONE_MAX_MATCHES=6 \
  mtg-duelstone-server
```

Startup takes about a minute while Forge loads its cards. `GET /api/health` answers when it's ready.

### HTTPS is required

The website is served over HTTPS, and browsers refuse an `http://` frame inside an `https://` page. Put the
engine behind a TLS proxy that passes WebSockets through. For example, with Caddy:

```
play.example.com {
    reverse_proxy 127.0.0.1:8080
}
```

The client calls the API and the WebSocket on the page's own origin, so the proxy needs no path rules.

### Point the website at it

Set `data-engine` on `#stage` in `website/index.html` to the server's origin, e.g.
`data-engine="https://play.example.com"`, and deploy the website. To try it locally before deploying, serve
`website/` and open it with `?engine=http://localhost:8080`. Only loopback addresses are accepted there.

## Sizing

Each running game holds one Forge game plus its AI threads. Measure with your own decks before raising the
cap: `cargo run -p mtg_client --bin autopilot -- --engine https://play.example.com --games 20` plays through
the same token flow as a visitor.
