# Identity and ownership without a database

## Google sign-in

Google OpenID Connect identifies a user by the verified token's `sub` claim.
Email is profile information, not the stable primary key: it can change.

The server uses authorization-code exchange with PKCE, a random state value, and
an OIDC nonce. The temporary verifier/state/nonce live in a short-lived signed,
HttpOnly, SameSite=Lax cookie. The code exchange happens on the server, where the
client secret remains private.

The ID token must pass signature, algorithm, issuer, audience, expiry, nonce, and
verified-email checks. Google's JWKS keys are cached in server memory and refreshed
when expired or when an unknown key ID appears. Google access/refresh tokens are
not persisted; the application only needs identity.

## Object-store profiles

The application user ID is derived from the Google subject:

$$
u=\texttt{"google-"}\;\Vert\;\operatorname{hex}(\operatorname{SHA256}(sub)).
$$

The profile is stored at `users/<u>.json`, containing its ID, display name,
email, optional picture URL, creation time, and last sign-in time. Existing
profiles are updated after verified sign-in. These are private objects accessed
by the server, not public bucket documents.

Hashing an identifier is not anonymization. Profile objects still contain personal
information and should follow the application's retention/access policies.

## Stateless application sessions

An application session contains a user ID and expiry, authenticated with
HMAC-SHA256. The cookie is HttpOnly, SameSite=Lax, and Secure on HTTPS. Signature
verification uses a constant-time MAC check. No session table or session object
is required.

Stateless sessions have a tradeoff: an individual stolen session cannot be
immediately revoked without introducing a revocation mechanism. Current sessions
expire after seven days; changing the signing secret invalidates all of them.
Per-user session epochs stored in profile objects are a possible future
revocation design if this becomes necessary.

Writes require `X-Ergo-Request: 1`. When present, the `Origin` header must match
the canonical HTTP(S) origin configured by `ERGO_PUBLIC_URL` (including scheme
and port). In local development with Google unset and a loopback public URL,
other loopback origins (`localhost`, IPv4 loopback, or `[::1]`) using the same
scheme are also accepted when their host and port match the request's `Host`
header. This supports alternate local names and development ports without
allowing cross-origin writes. Forwarded host headers are not used for this check.
Google-enabled and non-loopback deployments accept only the configured origin;
requests without `Origin`, such as native API clients, still need the custom
header and a permitted identity. OAuth state/nonce validation is separate from
normal application write protection.

### MCP user delegation

`POST /api/auth/mcp-token` accepts a signed-in browser session, the same-origin
write header/check, and `{ "expires_in_seconds": 3600 }` (the field is optional).
The response is `Cache-Control: no-store` and contains `token`, `expires_at`,
`audience`, and `scope`. The signed delegation uses a distinct purpose/actor
payload, so it cannot be exchanged for a session cookie or vice versa. Lifetime
is 1–86400 seconds and is capped at the issuing session's expiry. The audience
must match the canonical `ERGO_PUBLIC_URL`; server signing-secret rotation
invalidates outstanding delegations. Individual revocation is not implemented.

Shared-world GET routes accept the delegation as `Authorization: Bearer TOKEN`.
New tokens authorize world creation, assembly import, and geometry commits, still requiring
`X-Ergo-Request: 1` and checking any supplied `Origin`. Current owner/region policy
is rechecked at publication. Grant/retention changes, private snapshots, account
plans, and token issuance do not accept delegated authority. Older publication-only
tokens cannot perform geometry commits; the edit-token purpose is distinct. Invalid
bearer credentials do not fall back to a cookie or development identity on shared
routes. The native MCP bridge holds the configured token; it is never sent to the
local WASM client, included in tool arguments, or embedded in returned URLs.
See [MCP publication setup](local-construction-mcp.md#remote-publication-setup).

## Ownership and cached objects

Shared-world accepted audit records include the authenticated stable `actor` ID
and optional `actor_name`, resolved by the server from the account profile at
acceptance. Piece birth records likewise retain `created_by` and
`created_by_name`. The MCP client cannot choose these names. This is authenticated
author attribution in immutable content-addressed records, not a personal
cryptographic signature by a username. Permissions always use stable IDs; later
profile renames do not rewrite historical attribution. Legacy records may omit
the name. Display names are not unique usernames, and no Google email is copied
into the public-to-authorized-readers audit.

Each private manifest lives under its owner's prefix. Listing worlds reads the
shared example prefix and the current user's prefix. Reading a world or exporting
it checks ownership. Immutable blobs are globally content-addressed in storage,
but browser object URLs are scoped through an authorized world route.

The browser can retain downloaded private model bytes on that device. Persistent
caching is a local performance feature, not an access-grant mechanism. Logout
expires both session/login-state cookies and clears browser cache/storage state.
Both the viewer and signed-in setup page expose sign-out, which returns to `/login`
without waiting on CacheStorage calls. Missing/expired sessions can sign out too;
same-origin checks remain required. Session-status responses use `no-store`.
The service worker never
caches authentication responses or profile/world-list responses.

## Configuration

### Shared-world grants

`/api/shared-worlds/*` uses the same signed sessions and same-origin write header.
Creating a world establishes its owner. Existing-world reads require ownership or
an explicit user grant unless `authenticated_editing` is enabled. That opt-in
policy permits any signed-in identity to read/edit the world; otherwise writes
must lie in the grant's block-octree prefixes.
Only the owner may grant/revoke. Region grants authorize writes and world reads;
they do not promise regional read secrecy.

Policies are immutable CBOR versions selected by the same transaction root as
geometry. Publication validates its policy read, so concurrent revocation
invalidates a prepared edit. Google identity and profile last-write-wins fields
are never treated as permission assertions. Mesh delivery first resolves an
authorized piece in an accepted revision; knowing a content hash is not a grant.

Stable piece identities hash immutable creation records (including an instance
nonce and verified creating actor); pose/mesh edits select new immutable versions.
This is separate from Google user IDs and temporary local physics indices. See
[shared-world editing](concurrent-global-editing.md) for paging, operation budgets,
and model/retention semantics.

### OAuth environment

Create a Google OAuth **web application** client and register exactly:

```text
<ERGO_PUBLIC_URL>/auth/google/callback
```

Configure `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, a random
`ERGO_SESSION_SECRET` of at least 32 characters, and `ERGO_PUBLIC_URL`.
Outside localhost, the public URL must use HTTPS.

`ERGO_PUBLIC_URL` defaults to `http://127.0.0.1:3000` and must be an HTTP(S)
origin without credentials, a non-root path, query, or fragment. Host names and
default ports are normalized. If an import or save reports **Unexpected request
origin**, open the configured public URL or set `ERGO_PUBLIC_URL` to the browser's
origin and restart the server. For reverse-proxy deployments, use the external
browser-facing origin rather than the internal bind address.

With Google credentials unset, the server creates an explicitly named local
development profile in the same object store. This is a development mode, not
Google authentication. Configure Google before exposing a multi-user deployment.

## Scale and concurrency

Free shared-world edits are paced with a per-actor persistent token bucket,
defaulting to 5,000 changed bricks refilled per day. Operator-managed paid
entitlement bypasses pacing only. Account plans live separately from Google
profiles; profile last-write-wins updates cannot grant paid authority. See
[onboarding and pacing](access-and-onboarding.md).

Per-user prefixes avoid reading every user's manifests to answer one user's
catalog request. The current listing still reads all manifests for that owner.
Paginated immutable catalog pages and cached summaries can scale this without a
database.

Concurrent sign-ins may race when updating profile metadata. The current profile
contains no counters or authorization roles, so last-write-wins affects only
profile freshness/first-creation metadata. If profile fields later carry
security-sensitive state, use object-store conditional updates with ETags and
retries rather than relying on this behavior.
