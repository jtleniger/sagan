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

## Templates

Pages live in `assets/views/` and extend the shell in
`assets/views/layouts/app.html`. The layout chain, the context each page must
pass, static files, i18n, and the "add a new page" recipe are all in
[docs/templates.md](docs/templates.md).


## Getting help

Check out [a quick tour](https://loco.rs/docs/tutorials/the-tour/) or [build your first app](https://loco.rs/docs/tutorials/your-first-app/).
