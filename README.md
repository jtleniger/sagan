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

## Captures and periodic jobs

The `capture` job takes one still image from the camera and writes it to the
capture directory — `settings.capture.dir` in `config/<env>.yaml` (`captures/` by
default, gitignored; a test run writes `target/test-captures` instead). The camera
driver writes the file itself, the way the Pi's `libcamera-*` command-line tools do.

`capture` runs on the Captures interval saved on the Configuration page. The
scheduler runs one task, `periodic_work`, once a minute; that task asks each
registered job (`src/jobs/`) whether a slot is due, claims the slot in the
`job_runs` table, runs the work **inline in its own child process**, and records
the outcome. The stored interval therefore decides *whether* a slot is due, while
the YAML cron only decides how often the question is asked.

### Where the work runs

Three pieces, one of them configured:

| Piece | What it is | Chosen by |
|---|---|---|
| Scheduler | a clock: at each tick it runs a task as a subprocess | the `scheduler:` block in `config/<env>.yaml` |
| Task (`periodic_work`) | what a tick calls: it decides what is due and runs it | `src/tasks/periodic_work.rs` |
| Jobs (`capture`, …) | the work itself, run inline in that child process | `src/jobs::configured()` |

There is **no queue and no worker process**: the scheduler's child does the work,
and the outcome is a row in `job_runs` in the app database (`database.uri`). That
is enough here — one device, one camera, work that is periodic rather than
request-driven — and it removes the second SQLite file, the second pool and the
"is my worker up?" question. What it costs: in-flight work does not survive a
restart, and the interval retry is the recovery.

**Run exactly one scheduler process.** Two would double-claim slots; the unique
`(job, slot_at)` index and the running-guard catch same-tick overlap, but they are
a safety net, not a design. `workers.mode` is `ForegroundBlocking`, which is also
why no provider is needed: nothing is ever enqueued.

```sh
cargo loco start --all                  # one process: server + scheduler
scripts/dev-split.sh                    # two: web;  and scheduler
cargo loco start                        # web only — nothing runs the jobs
cargo loco start --worker --scheduler   # a scheduler, no HTTP server
```

### The `/jobs` page

Two tables. **Periodic jobs** is one row per registered job: what it does, the
cadence as configured now, when it last ran (status and age) and when it is next
due. A highlighted row means the job's newest run is at an older slot than the one
due now — the dispatcher is behind — which is the whole of "is it running?" now
that the work *is* the task.

**Runs** is the `job_runs` history, newest slot first, paginated 25 to a page: job,
status (`running` / `succeeded` / `failed`), the schedule slot, the start time, the
elapsed time and the detail (the capture's filename, or the error). That table
is also the due rule's state: the newest row per job says when it last ran, and a
`running` row with a fresh `started_at` is what keeps the next tick off a slow job.
A failed run has consumed its slot, so it is not retried until the *next* slot.
History is kept for 30 days; the prune that runs at the end of each dispatch always
keeps the newest row per job, because deleting it would make the job fire on the
very next tick.

## Templates

Pages live in `assets/views/` and extend the shell in
`assets/views/layouts/app.html`. The layout chain, the context each page must
pass, static files, i18n, and the "add a new page" recipe are all in
[docs/templates.md](docs/templates.md).


## Getting help

Check out [a quick tour](https://loco.rs/docs/tutorials/the-tour/) or [build your first app](https://loco.rs/docs/tutorials/your-first-app/).
