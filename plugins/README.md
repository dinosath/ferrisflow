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
