# offline/

The complete offline pack for the game and its build. Desktop builds prefer this pack and download
missing card images automatically into the user data folder (`offline/cache`), keeping them for offline
reuse. Filling the full pack is optional; it lets you view previously unseen cards without internet.
An engine started directly stays offline unless given `--image-cache WRITABLE_DIR`.

Fill it once, while online:

```bash
tools/fetch-offline.sh                 # everything below (~7–10 GB, resumable)
tools/fetch-offline.sh --images-only   # just refresh card images / rulings / showcase
```

| Path | In git | What | Used by |
|---|---|---|---|
| `data/rulings.json` | yes | Scryfall rulings, one card per line | engine `GET /api/rulings` |
| `data/showcase.json` | yes | curated showcase printings + artist artwork counts | splash showcase (`mtg_client::scryfall`) |
| `img/` | no | one printing per card: `large/` + `art_crop/` JPEGs, `index.json` (name → file), `missing.txt` (Forge names without an image) | engine `GET /api/img`, splash showcase (`img/showcase/`) |
| `img/.bulk/` | no | the raw Scryfall bulk JSON the fetch works from | `fetch-offline` |
| `vendor/` | no | every Rust crate (`cargo vendor`) | cargo, via the generated `.cargo/config.toml` |
| `m2/` | no | Maven repo: Forge modules, engine dependencies, plugins | Maven, via `MAVEN_ARGS` in `tools/env.sh` |
| `forge-<tag>.tar.gz` | no | the pinned Forge source | `tools/setup-forge.sh` when `./forge` is missing |
| `toolchain/` | no | JDK 17 + Maven tarballs | `tools/install-toolchain.sh` |

Card images are © Wizards of the Coast. They are never committed or shipped in public releases;
each installation fetches its own copy.

The engine finds this folder with `--offline DIR` (the game passes it) or by walking up from its
working directory. The game also looks in `$MTG_DUELSTONE_OFFLINE`, next to a packaged build, and
in `~/.local/share/mtg-duelstone/offline` (where `fetch-offline` writes outside a checkout).

Not covered: the Rust toolchain itself (rustup), system development libraries (ALSA, udev,
Vulkan), and Python for the asset-generation scripts in `crates/arena/assets/tools/`.

A dependency added after the fetch makes the offline cargo build fail. Re-run
`tools/fetch-offline.sh` while online to vendor it.
