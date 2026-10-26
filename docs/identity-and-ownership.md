# Identity and private account state

The server validates Google OpenID Connect in the human's browser, including the
configured audience/nonce and verified claims. It issues signed HttpOnly session
cookies and stores the stable account profile privately in object storage.
Agents receive short-lived, purpose/origin-bound delegated tokens instead of
Google credentials or S3 keys.

## Records and attribution

`users/<user-id>.json` stores ID, name, email, optional picture and login timestamps.
Authorization uses the stable ID. At acceptance the server resolves a display
name into the Audit and creation Birth; names supplied by the client are not
trusted attribution. Display names are not unique identities or personal signatures.

The public Sandbox exposes accepted construction data and its attribution.
Profiles, private last positions, pacing ledgers, account plans and configuration
backups remain private. S3 itself is not anonymously readable.

## Sessions and delegated access

Signed session cookies expire; signing-key rotation invalidates signatures.
Logout clears the browser session/login cookies and model caches and redirects to
setup. It is idempotent and does not revoke already-issued agent delegations.

Current delegated tokens carry the actor, audience, purpose and expiry. The server
accepts the current edit purpose only. They cannot become browser sessions, issue
another token, change sharing/grants, retention or operator-owned entitlement.
Writes require the custom same-origin header and validate browser Origin when present.

## Shared-world permissions

The accepted Policy contains owner, region grants, authenticated-editing setting
and optional retention. A prepared edit watches the relevant policy/model versions;
revocation invalidates publication. Shared assembly edits preserve creation IDs
and validate their complete affected dependency domain before head CAS.

## Last positions and plans

`user-positions/<actor-hash>.json` stores the caller's native navigation context.
The API derives the actor from authentication and exposes no arbitrary-user read
parameter. Conditional writes and per-browser sequences reject delayed observations.
The MCP receives only its user's saved point and derived ground-level transform.

Pacing and `account-plans` are independent of identity/profile and world geometry.
An unlimited plan is server-owned, persists across cold starts and enables god
mode: no pacing quota or authoring brick-count cap. It does not authorize a region
or validate a construction. Byte/request budgets remain independent of entitlement.

See [record formats](object-store-formats.md), [access](access-and-onboarding.md)
and [shared editing](concurrent-global-editing.md).
