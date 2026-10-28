# Scaleway deployment

The hosted service at https://ergosandbox.com/ uses one scale-to-zero API/workbench
container and the existing private `ergo` S3 bucket in `fr-par`. The public guide
at https://www.ergosandbox.com/ is GitHub Pages.

## Existing stack

`infra/scaleway` is the implemented OpenTofu module for namespace, container,
environment, domain and DNS. The common world is configured with
`common_world_id`/`ERGO_COMMON_WORLD_ID`; `world_prefix`/`ERGO_WORLD_PREFIX=world`
selects the authoritative repository under `S3_PREFIX=ergo`.

Existing deployment identifiers are maintained in the private state/variable
files. Restore that state before managing the stack on another machine. Secrets,
state, saved plans and `.env` are ignored and never belong in a source commit.

## Inputs

- Scaleway project/region and provisioning credentials.
- Verified image digest, resource/memory/scale inputs and existing bucket name.
- Application S3 access/secret, region, endpoint and prefix.
- Google Web client ID/secret, stable session-signing key and public origin.
- Common world ID/prefix, brick-change pacing and map-provider configuration.
- `request_timeout_seconds`: container processing deadline, default 300 seconds,
  configurable from 1 to 1800. Diagnosed long validated edits can use a larger
  value; native MCP/client deadlines are separate. Longer deadlines do not retry
  mutations or relax validation, permissions, or working-set budgets.

Provisioning credentials are separate from application S3 credentials. Private
tfvars override `TF_VAR_*` environment defaults; keep their values consistent
when planning/rotating. Use the example variable file for a new isolated setup.

## Image and deployment

Build/test/publish the image with `release.yml`. A tag publishes complete client
and image distributions; dispatch `publish_container=true` for image-only CI.
The deployment pins the resulting Linux/AMD64 GHCR digest, not a mutable guess.

```sh
tofu -chdir=infra/scaleway init -input=false
tofu -chdir=infra/scaleway validate
tofu -chdir=infra/scaleway test
tofu -chdir=infra/scaleway plan -input=false -var="image=$IMAGE" -out=deployment.tfplan
tofu -chdir=infra/scaleway apply -input=false deployment.tfplan
```

For a first deployment, bootstrap with `bind_custom_domain=false`, apply, resolve
the returned alias target publicly, then plan/apply the bound domain. Updates use
the existing state. Current-format history cutover is coordinated under its world
barrier as described in [operations](operations.md).

## DNS, Google and HTTPS

The apex ALIAS points to the container's returned domain. `www` is a CNAME to
`ngrislain.github.io.`. The public guide uses its Pages CNAME/managed certificate;
the API origin remains the apex. Both use managed HTTPS.

In the actual Google OAuth Web application, set this exact authorized redirect:

```text
https://ergosandbox.com/auth/google/callback
```

It is a redirect URI, not a JavaScript origin. Homepage/privacy/terms branding
uses the `www` guide. Complete Google sign-in in the browser after deployment and
check `/api/config`, native format fields, one Sandbox, private position access,
MCP accepted read-back, allowance and clean shutdown/scale behavior.

The private bucket-root configuration backup is `_ops/ergo/production.env`.
It is a recovery artifact, not an alternate database or a substitute for state.
