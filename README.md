# crux-torrent

A CLI BitTorrent (v1) client written in Rust, built as a from-scratch learning project. It parses
`.torrent` files, announces to an HTTP tracker, pulls pieces from the swarm over the peer wire
protocol, and streams verified pieces to disk through a dedicated async disk-writer actor.

It is a downloader, not a full client: there is no seeding, DHT, or UDP tracker support
yet. See the [feature checklist](#feature-checklist) below for the exact boundary.

> Linux only - the disk writer relies on the Linux `pwritev` syscall (see [Requirements](#requirements)).

## Table of contents

- [Feature checklist](#feature-checklist)
- [Requirements](#requirements)
- [Installation](#installation)
- [Usage](#usage)
- [Testing](#testing)
  - [Unit tests](#unit-tests)
  - [Integration swarm (`test_network/`)](#integration-swarm-test_network)
- [Tracing / diagnostics](#tracing--diagnostics)

## Feature checklist

- [x] Bencode metainfo (`.torrent`) parsing
- [x] HTTP tracker announce 
      uses `udp://`
- [x] Peer handshake (BEP 3)
- [x] Peer wire message codec
- [x] Rarest-first piece selection across all connected peers
- [x] Per-piece download de-duplication 
- [x] Block-level request pipelining with per-peer adaptive window sizing
- [x] Stale/timed-out block request requeueing
- [x] SHA-1 piece hash verification
- [x] Piece-to-file routing for multi-file torrents (pieces spanning file boundaries)
- [x] optimized write coalescing, threshold/age-based disk flushing via `pwritev`
- [x] `tracing`-based structured logging
- [x] `tracing-flame` profiling export
- [ ] Migrate to compio (would make windows support easier)
- [ ] Windows Support
- [ ] Endgame mode
- [ ] Resume support
- [ ] UDP tracker support (BEP 15)
- [ ] Seeding / uploading 
- [ ] Choking algorithm (tit-for-tat, optimistic unchoke) 
- [ ] Multi-tracker / announce-list fallback 
- [ ] Distributed Hash Table (DHT) support(BEP 5) 
- [ ] Magnet links / extension protocol 
- [ ] Bandwidth/rate limiting

## Requirements

- Rust (stable toolchain; see `Cargo.toml` for the 2021 edition)
- **Linux** (requires the pwritev syscall for disk, may expand to support windows in the future)
- [Docker](https://docs.docker.com/get-docker/) + Compose, and `mktorrent` — only needed for the
  integration swarm described under [Testing](#testing)

## Installation

```sh
git clone <this repo>
cd crux-torrent
cargo build --release 
```

## Usage

```
crux-torrent [OPTIONS] <SOURCE>
```

| Argument/Flag        | Description                                                             | Default |
|-----------------------|--------------------------------------------------------------------------|---------|
| `<SOURCE>`            | path to the `.torrent` file to download                                  | required |
| `-p, --port <PORT>`   | port to listen on                                                        | `8860`  |
| `-o, --output-dir <DIR>` | directory downloaded file(s) are written into                        | `.`     |
| `-v, --verbose`       | repeatable; `-v` = info, `-vv` = debug, `-vvv` = trace                   | warn-level logging |
| `-q, --quiet`         | drop logging down to errors only (conflicts with `-v`)                  | off     |
| `-g, --generate-trace`| write a `tracing-flame` folded stack file to `./tracing.folded`          | off     |
| `-h, --help`          | print help                                                               |         |

Example:

```sh
# if installed using cargo install, use the binary directy instead of cargo run
cargo run --release -- --output-dir ./downloads -v ./some-torrent-file.torrent 
```

The process runs until every piece has been downloaded and flushed to disk, or until it's
interrupted with Ctrl-C (which cancels in-flight work and flushes whatever has already been
verified before exiting).

## Testing

### Unit tests

```sh
cargo test
```
### Integration swarm (`test_network/`)

`test_network/` holds a Docker Compose setup that stands up a real BitTorrent tracker
([opentracker](https://erdgeist.org/arts/software/opentracker/)) plus a swarm of seeding
[Transmission](https://transmissionbt.com/) peers, so the client can be exercised against real
peers instead of mocks. None of the generated torrent/data files are committed
(`test_network/.gitignore` excludes everything matching `test*`); you generate them locally.

**Single-file swarm:**

```sh
cd test_network
# create a file to seed, and a torrent for it, pointing at the tracker started below
head -c 200M </dev/urandom > testfile
mktorrent -a http://172.30.0.1:6969/announce -o test.torrent testfile

docker compose up --wait     # starts the tracker + 20 seeding transmission peers
```

**Multi-file swarm** (files under `testdir/`, including a nested subdirectory, on the `multi`
compose profile):

```sh
cd test_network
mkdir -p testdir/sub
head -c 80M  </dev/urandom > testdir/a.bin
head -c 100M </dev/urandom > testdir/c.bin
head -c 20M  </dev/urandom > testdir/sub/b.bin
mktorrent -a http://172.30.0.1:6969/announce -l 18 -o test-multi.torrent testdir

docker compose --profile multi up --wait
```

Then, from the repo root, run the client against the generated torrent file — the tracker is
reachable at `172.30.0.1:6969` on the compose network, which Docker's bridge driver routes to
automatically from the host:

```sh
cargo run --release -- -v --output-dir /tmp/crux-download test_network/test.torrent
```

Tear the swarm down with:

```sh
cd test_network
docker compose down          # or: docker compose --profile multi down
```

## Tracing / diagnostics

Pass `-g`/`--generate-trace` to emit `./tracing.folded`, a folded-stack file compatible with
[inferno](https://github.com/jonhoo/inferno) / flamegraph tooling:

```sh
cargo run --release -- -g ./some-torrent-file.torrent
inferno-flamegraph < tracing.folded > flame.svg
```
