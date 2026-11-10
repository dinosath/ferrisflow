# Plugin resources and hooks

A plugin declares its installed prerequisites with `dependencies`, for example
`[{"id":"ferrisflow.content-types","version":"^1.0.0"}]`. Imports check the
installed, enabled version. Dependency downloading and installation are separate.

Plugins extend manifests through `resourceTypes`. A type declares its `name`, a
`create` workflow, and optionally a `postInstall` workflow. Other plugins supply
`resources: [{"type":"content-type","definition":{...}}]`. A provider may also
specify a `source` path such as `database.contentTypes` for an existing manifest
collection. Consumers explicitly depend on the provider. Unknown types fail.

Import saves the plugin and its workflows, runs each resource's create workflow,
then its post-install workflow. Finally it runs the importing plugin's optional
`hooks.postInstall` workflow. Resource hooks receive `definition`, `resource`, and
`plugin`. All database and workflow changes share one transaction; a failing hook
rolls back the import. Reapplying a package updates workflows by document identity.

`ferrisflow.content-types.json` implements content types using OWS workflows. Its
create handler validates the schema, creates the physical table, and registers the
schema. Its post-install handler generates CRUD workflows from JSON templates.
`local.novels.json` supplies novels and chapters definitions and the existing
crawler workflows. `example.novels.json` is a smaller package for experimentation.
Neither consumer supplies CRUD implementations.

Workflows call `ferrisflow.primitive` with a `primitive` argument:

- `database`: `ensure`, `list`, `read`, `create`, `update`, `upsert`, or `delete` on
  a named `table`. `ensure` takes typed `columns`; record operations take `input`
  and optionally `primaryKey` and `key`. `jsonFields` maps JSON properties to text
  columns. `filter` binds equality conditions; `fallback` selects another database
  operation if the requested table does not exist. This lets plugin workflows
  keep access to records in older storage layouts. `ignoreFields` removes routing fields. `returnInput` preserves input
  for the next task. Identifiers are checked and values are bound parameters.
- `workflow.save`: validate and save an OWS `definition`.
- `workflow.instantiate`: load a named `template` from an installed `plugin`'s
  `workflowTemplates`, substitute `parameters`, then validate and save each
  definition. `{{name}}` substitutes string parameters; `{"$parameter":"name"}`
  substitutes any JSON value. Template expressions remain workflow expressions.

Data tables lists actual database tables and displays their records using the
shared DataTable component. Plugin datatable views can use the generated REST
routes at `/api/collections/<content-type-uid>` and the generated workflow names.
The current database list primitive returns at most 1,000 records.

## Novels plugin

`local.novels.json` version 0.2.0 requires the enabled content-types provider at
version 1.1.x or later within major version 1. Starting the updated server
registers the bundled provider automatically. Then update the existing
`local.novels` package through Plugins, or import it for a fresh installation. `example.novels.json` remains a small independent
example with its own `novels` collection; installing both packages is supported.

The Novels page provides **Import novel**, **Refresh**, **Edit**, and **Delete**
actions. Chapters provides **Read**, **Edit**, and **Delete**. Read displays
sanitized chapter HTML in the workflow dialog. Record creation and editing use
forms generated from the collection schema; rich text uses a multiline input.
Workflow results and failures stay visible until the dialog is closed.

Import accepts a Brightnovels series URL and optional integer `chapter_from` and
`chapter_to` bounds, defaulting to chapters 1–5. `auto_update` defaults to false
for a new novel. Refresh loads the saved checkpoint and starts at the following
chapter unless an explicit `chapter_from` is supplied. Both workflows exclude
premium chapters and validate URLs and chapter ranges before network requests.
The parser expects Brightnovels' embedded `data-page` payload and chapter-list
response; tests use fixtures for those formats, without contacting the live site.

Each successful crawl saves the novel and all downloaded chapters in one
transaction. Unique indexes on novel `source` and chapter `source_url` make
reimports update existing records. Checkpoints cannot move backwards, and a
refresh that did not explicitly change `auto_update` preserves the current
stored preference. Failed downloads or failed writes leave the checkpoint and
records unchanged. Successful runs populate `source_site`, `last_checked_at`,
and chapter `downloaded_at` in UTC. Chapter numbers and checkpoints are integers.

The `refresh-auto-novels` OWS workflow runs every 30 minutes while the server is
running, refreshing only opted-in novels. A failure for one novel does not stop
the others. The server discovers schedule changes within five seconds and stops
future runs when the owning plugin is disabled or removed. Schedules run in the
server process; missed intervals while the server is offline are not replayed.
Run one scheduler instance per database to avoid duplicate network requests.
The existing list primitive's 1,000-record limit also bounds the auto-refresh
selection. Existing tables receive the new unique indexes on package update;
pre-existing duplicate source URLs must be resolved before that update succeeds.

The two crawl workflows are also exposed as `POST /api/novels/import` and
`POST /api/novels/refresh`. For example, import with this JSON body:

```json
{
  "novel_url": "https://brightnovels.com/series/your-series-slug",
  "chapter_from": 1,
  "chapter_to": 5,
  "auto_update": true
}
```

Additional reusable primitives used by the package:

- `database.batch`: run an array of database `operations` in one transaction;
  `returnInput` returns the supplied `input` after commit.
- `database` upserts accept a unique `conflictKey`, `maxFields` for values that
  may only increase, and `preserveFields` to leave stored values unchanged on
  conflict. `ensure` columns can request a `unique` index, including on an
  existing table.
- `clock.now`: return the current UTC timestamp in RFC 3339 format.

Native OWS `run.workflow` tasks can call other active, installed workflows.
Datatable `config.actions` and `config.rowActions` expose named workflows;
`inputMapping` maps input property names to dotted paths in the selected row.
