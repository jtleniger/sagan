# Server-rendered templates

Everything this app serves is Tera-rendered HTML plus CDN `<script>` tags. No
JS framework, no bundler, no build step. This file is the contract every new
page follows.

## Where templates live, and how a request finds one

Templates live under `assets/views/`, and the **template key is the path
relative to that directory**:

|File|Key|
|---|---|
|`assets/views/base.html`|`base.html`|
|`assets/views/auth/login.html`|`auth/login.html`|
|`assets/views/layouts/app.html`|`layouts/app.html`|
|`assets/views/dashboard/index.html`|`dashboard/index.html`|

The engine is built by `ViewEngineInitializer` (`src/initializers/view_engine.rs`),
which `src/app.rs` registers in `Hooks::initializers`. In `after_routes` it builds
a `TeraView` from `assets/views` and layers `Extension<ViewEngine<TeraView>>` onto
the router. Handlers take that extension back out through the extractor:

```rust
#[debug_handler]
async fn index(ViewEngine(v): ViewEngine<TeraView>) -> Result<Response> {
    format::render().view(&v, "dashboard/index.html", data!({"user": user}))
}
```

- `format::render()` starts a response builder: `.status(code)`, `.cookies(..)`,
  then exactly one finalizer — `.view(&v, key, data)`, `.redirect(to)`, `.json(..)`,
  `.text(..)`, `.empty()`.
- `data!` is `serde_json::json!` re-exported by `loco_rs::prelude`. The value only
  has to be `Serialize`; `src/views/` holds the DTOs templates get, never a
  `users::Model` (the entity carries the password hash and API key).

Autoescaping is on for `.html`, so `{{ user.name }}` is HTML-escaped. Use
`| safe` only for markup you built yourself.

## The layout chain

```
base.html                  <html>, <head>, Tailwind CDN script, <body>
└── layouts/app.html       top bar (user + sign out) and left nav
    └── dashboard/index.html   page-specific content
```

`base.html` is the only file with `<html>`/`<body>`. `layouts/app.html` extends
it and is the shell **authenticated pages extend** — it renders the top bar and
the left navigation, and exposes three blocks:

|Block|Filled by|
|---|---|
|`meta_title`|`<title>` text (default `Sagan`)|
|`nav`|the sidebar links (default: Dashboard)|
|`content`|the page body|

A page's context contract with the shell is two keys:

```json
{ "user": { "pid": "...", "name": "...", "email": "..." }, "active": "dashboard" }
```

`user` is `views::user::UserView`; `active` is the nav key (`"dashboard"`,
`"system"`, `"logs"`, `"configuration"`) that `layouts/app.html` compares to
highlight the current link. Pages that render through the shell must pass both.

Tera 2 resolves `{% extends %}` and `{% block %}` when templates are **loaded**,
so a child whose parent does not exist (or a block it never defines) fails at
boot, not on the first request. `{% include %}`, `{% macro %}`, filters
(`{{ name | upper }}`) and `{{ super() }}` all work as documented at
<https://keats.github.io/tera/docs/>.

## Static files and i18n

- URL `/static/...` maps to `assets/static/...` (`server.middlewares.static` in
  `config/*.yaml`; missing files fall back to `assets/static/404.html`).
- The same initializer registers a Fluent `t()` function, so templates can call
  `{{ t(key = "greeting") }}`. Strings live in `assets/i18n/<locale>/main.ftl`
  (`en-US` is the default locale) plus the shared `assets/shared.ftl`. No current
  template needs it; it is there for the first page that does.
- In debug builds the engine watches `assets/views` and re-reads a changed
  template, so editing HTML while `cargo loco start` runs is enough. Release
  builds embed nothing — the files are read at boot.

## Adding a page

The generator writes the file *and* the wiring; Rust has no autoloading, and
hand-wiring is how "the handler exists but 404s" happens.

1. `cargo loco generate controller reports` — writes `src/controllers/reports.rs`,
   declares the module in `src/controllers/mod.rs`, and adds
   `.add_route(controllers::reports::routes())` in `src/app.rs`.
2. Replace the generated stub with a handler that renders under the shell:

   ```rust
   #[debug_handler]
   async fn index(
       ViewEngine(v): ViewEngine<TeraView>,
       auth: Option<auth::JWT>,
       State(ctx): State<AppContext>,
   ) -> Result<Response> {
       let Some(auth) = auth else {
           return format::redirect("/login");
       };
       // ... load what the page needs ...
       format::render().view(
           &v,
           "reports/index.html",
           data!({"user": UserView::from(&user), "active": "reports"}),
       )
   }

   pub fn routes() -> Routes {
       Routes::new().add("/reports", get(index))
   }
   ```

3. Add `assets/views/reports/index.html`:

   ```html
   {% extends "layouts/app.html" %}

   {% block meta_title %}Reports · Sagan{% endblock %}

   {% block content %}
   <h1 class="text-2xl font-semibold tracking-tight">Reports</h1>
   {% endblock %}
   ```

4. Add the sidebar link to `layouts/app.html`'s `nav` block — that block is what
   every page renders, so a link added there is reachable from all of them —
   and add the page's nav key to its `active`:

   ```html
   <a href="/reports"
      class="block rounded-md px-3 py-2 text-sm font-medium {% if active == "reports" %}bg-slate-100 text-slate-900{% else %}text-slate-600 hover:bg-slate-50{% endif %}">Reports</a>
   ```

   A `nav` override in a page template (with `{{ super() }}` to keep the
   existing links) only affects that one page; the shell is where a link goes.

5. `cargo test` — `tests/views/render.rs` renders templates without a server, so
   a Tera mistake (bad key, missing parent, unknown function) fails there in
   milliseconds instead of at request time.

## Styling

Tailwind v4 is loaded from the jsDelivr CDN by the `<script>` tag in `base.html`
and compiles classes in the browser. That is deliberate for this stage: no build
step, no `node_modules`. The production swap is a compiled
`assets/static/css/app.css` built with the Tailwind CLI, referenced from the
`head` block instead — out of scope here, and `base.html` is the single place it
touches.

### The base layer

`base.html` carries one `<style type="text/tailwindcss">` block, outside
`{% block head %}` so that a page overriding that block without `{{ super() }}`
cannot drop it. It holds the rules that belong to every element of a kind rather
than to one page — today, the pointer cursor on controls:

```css
@layer base {
  button:not(:disabled), select:not(:disabled), input[type="datetime-local"]:not(:disabled) { … }
}
```

A browser's own default for `button` and `select` is the arrow cursor, and
Tailwind v4's preflight — unlike v3's — deliberately leaves it alone, so every
button used to need `cursor-pointer` written on it by hand. Extend that block
rather than repeating a utility; `@layer base` is what keeps a `cursor-*` utility
able to override it for a single element. CSS that is not Tailwind-compiled goes
in a plain `<style>` — an unlayered rule outranks every utility class.

## Dates and times

**The server speaks UTC; the browser speaks the reader's zone.** Every timestamp
the backend stores, filters on, or renders is UTC, and so are the `?from=`/`?to=`
values it parses. Showing them locally, and converting back before the form is
submitted, is the browser's job; `assets/static/js/local-time.js` renders the
server's instants in the reader's zone, `assets/static/js/logs.js` converts a form
boundary back, and these are the three hooks:

|Markup|Contract|
|---|---|
|`<time datetime="…Z" data-local-time>UTC text</time>`|`datetime` carries the instant; the script rewrites the text in the reader's locale and puts the instant in `title`. The UTC text is the fallback for a reader without the script.|
|A group with an `id` and `data-utc="YYYY-MM-DDTHH:MM:SS"`|The reader-facing half of a filter boundary: a `-date` and a `-time` input, which the script fills from `data-utc` on the reader's own clock, and a `-utc` input, which is the only part of the boundary submitted. All three are found by the group's `id` plus that suffix.|
|The `-utc` input (`<input type="hidden" name="from">`)|Starts holding the server's UTC instant, so the range survives a submit of the form's other fields with or without the script. On submit the script rewrites it from the two controls — or empties it, which is what clears the filter — so exactly one `from`/`to` reaches the query string.|

Two controls rather than one `datetime-local`, because Firefox gives that input
no time picker. The cost of the split is that the reader-facing controls carry no
`name`: a page whose script never loads shows the UTC instant but cannot change
it. That is the trade — a boundary is the script's to submit, and the backend's
to read as UTC.

A DTO backing such a page therefore carries both ends of the instant:
`LogEntryView::timestamp` (UTC text, the fallback) and
`LogEntryView::timestamp_utc` (RFC 3339, what the script reads) — and, for a
boundary, `LogsBoundaryView`'s `date`/`time`/`utc`.
