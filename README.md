# Welcome to Loco :train:

[Loco](https://loco.rs) is a web and API framework running on Rust.

This app is **server-rendered**: pages are Tera templates under
`assets/views/`, styled with Tailwind from a CDN. Authentication is a JWT in an
HttpOnly cookie, set by the login page.

## Logging in

```sh
cargo loco start   # migrate happens on boot; migrate by hand with `cargo loco db migrate`
```

Open <http://localhost:5150/> → redirected to `/login`. The admin row is created
by the `seed_default_user` migration, so a fresh database is enough to sign in:

|Field|Value|
|---|---|
|Email|`admin@example.com`|
|Password|`admin`|


## Quick Start

```sh
cargo loco start
```

```sh
$ cargo loco start
Finished dev [unoptimized + debuginfo] target(s) in 21.63s
    Running `target/debug/myapp start`

    :
    :
    :

controller/app_routes.rs:203: [Middleware] Adding log trace id

                      ▄     ▀
                                 ▀  ▄
                  ▄       ▀     ▄  ▄ ▄▀
                                    ▄ ▀▄▄
                        ▄     ▀    ▀  ▀▄▀█▄
                                          ▀█▄
▄▄▄▄▄▄▄  ▄▄▄▄▄▄▄▄▄   ▄▄▄▄▄▄▄▄▄▄▄ ▄▄▄▄▄▄▄▄▄ ▀▀█
 ██████  █████   ███ █████   ███ █████   ███ ▀█
 ██████  █████   ███ █████   ▀▀▀ █████   ███ ▄█▄
 ██████  █████   ███ █████       █████   ███ ████▄
 ██████  █████   ███ █████   ▄▄▄ █████   ███ █████
 ██████  █████   ███  ████   ███ █████   ███ ████▀
   ▀▀▀██▄ ▀▀▀▀▀▀▀▀▀▀  ▀▀▀▀▀▀▀▀▀▀  ▀▀▀▀▀▀▀▀▀▀ ██▀
       ▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
                https://loco.rs

environment: development
   database: automigrate
     logger: debug
compilation: debug
      modes: server

listening on http://localhost:5150
```

## Hardware

The Raspberry Pi subsystems — a PWM case fan, an I2C BME280, the camera — live
behind the traits in `src/hardware/`. Every host gets a bundle at boot, chosen by
`settings.hardware.driver` in `config/<env>.yaml`:

|Value|Meaning|
|---|---|
|`auto` (default)|The Pi drivers where the build has them, the mock everywhere else|
|`mock`|The mock on any host — including a Pi whose hardware is being serviced|
|`pi`|The Pi drivers, or a boot error in a build that has none|

The mock is compiled for every host and is what a laptop and CI run; the Pi
drivers (`src/hardware/pi.rs`) are not written yet, so `driver: pi` fails the
boot rather than silently driving nothing.

Probe what the running build actually has, without a browser:

```sh
cargo loco task hardware_check            # frames land in target/, override with dir:/tmp
```

It reads the BME280, sets the fan to half duty and reads it back, and writes one
capture; a subsystem this host does not have is reported and exit status stays 0.

The `/system` page's Hardware card shows the fan's commanded duty and the
sensor's humidity and pressure, refreshed with the rest of the live panel.

## Captures and jobs

The `CaptureWorker` takes one still image from the camera and stores it in the
app's single file store — the local driver rooted at `settings.storage.dir` in
`config/<env>.yaml` (`captures/` by default, gitignored; a test run writes
`target/test-captures` instead).

Nothing captures on a timer by itself. The scheduler runs the `enqueue_capture`
task once a minute, and that task enqueues a capture only when the Captures
interval saved on the Configuration page is due — so a capture happens only while
the scheduler process is running. A second entry, `heartbeat`, runs every minute
too: it records that the scheduler ticked, and enqueues a job that records a
worker drained the queue, which is what `/jobs` shows as *Runtime*.

### Where the work runs

Four pieces, and only the first three are configured:

| Piece | What it is | Chosen by |
|---|---|---|
| Scheduler | a clock: at each tick it runs a task as a subprocess | the `scheduler:` block in `config/<env>.yaml` |
| Task (`enqueue_capture`) | what a tick calls; it reads the interval and puts a *job* in the mailbox | your code |
| Queue | the mailbox: one row per job in `sqlt_loco_queue`, inside the SQLite file `queue.uri` names | `workers.mode: BackgroundQueue` + `queue.kind: Sqlite` |
| Worker | a *process* that polls the mailbox and runs the job | the `cargo loco start` flags |

So a job outlives the process that enqueued it, and the worker is a runtime
choice, not a setting:

```sh
cargo loco start --all              # one process: server + worker + scheduler
scripts/dev-split.sh                # two: web;  and worker + scheduler
cargo loco start                    # web only — jobs queue up, nothing runs them
cargo loco start --worker           # a worker process, no HTTP server
cargo loco start --worker --scheduler   # worker + scheduler, no HTTP server
```

Use the split shape (or your own supervisor) when the work must not share
threads with requests, or must survive a web restart. Keep *one* worker process
against a SQLite queue: the file takes one writer at a time.

### The `/jobs` page

The page shows five things: the worker mode and the queue provider's own
description and ping; whether the two *processes* are alive (`Runtime`); the
workers this app registers; the scheduler's entries (`Scheduled` — what will be
asked for, and when); and the queue's rows (`Jobs` — status, elapsed, tags,
newest first).

`Runtime` is the answer to "is my worker up?", which nothing else on the page can
give: the provider's `Reachable.` line only means the queue *file* answers, and a
web-only process reports that too. A process cannot see another process's worker
loop — Loco keeps the loop's cancellation token inside its queue provider, and the
`Queue` handle hides the provider — so the work stamps a row in
`runtime_heartbeats` as it runs (`src/models/runtime_heartbeats.rs`). `live` means
a stamp arrived within two ticks, `stale` means it stopped, `missing` means it never
started. When rows are waiting *and* no worker has stamped recently, the page says
so in a red banner at the top — that combination is the one thing here a reader has
to act on.

Two databases have to be shared for any of that to line up: the stamps live in the
app database (`database.uri`) and the queue in its own file (`queue.uri`). In a split
deployment, point both processes at the same two — a worker writing stamps into a
different database would look, from the web process, exactly like a worker that
never started. History is kept for a day and pruned by the next stamp, which is what
makes a gap visible after the fact.

Two buttons steer the queue: **Cancel** on a job that has not started, and
**Retry** on one that failed, plus a page-level **Requeue jobs stuck over 5
minutes** for rows left in `processing` by a worker that died mid-job. A job that
is *running* cannot be cancelled — the process performing it never re-reads its
row — so stopping running work means stopping the worker process. Elapsed time is
derived from the queue's own timestamps; Loco stores no per-job output, so job
logging lives on `/logs`.

The page reads the queue through Loco's own `bgworker::sqlt` helpers on a second
pool over the same SQLite file (see `src/queue.rs`); the same rows are on the CLI
with `cargo loco jobs dump --folder target`, `jobs retry`, `jobs requeue`,
`jobs cancel` and `jobs tidy`.

## Templates

Pages live in `assets/views/` and extend the shell in
`assets/views/layouts/app.html`. The layout chain, the context each page must
pass, static files, i18n, and the "add a new page" recipe are all in
[docs/templates.md](docs/templates.md).


## Getting help

Check out [a quick tour](https://loco.rs/docs/tutorials/the-tour/) or [build your first app](https://loco.rs/docs/tutorials/your-first-app/).
