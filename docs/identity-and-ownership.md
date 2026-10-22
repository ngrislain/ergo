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

Authenticated writes require a custom same-origin request header and reject a
mismatched Origin header. OAuth state/nonce validation is separate from normal
application write protection.

## Ownership and cached objects

Each private manifest lives under its owner's prefix. Listing worlds reads the
shared example prefix and the current user's prefix. Reading a world or exporting
it checks ownership. Immutable blobs are globally content-addressed in storage,
but browser object URLs are scoped through an authorized world route.

The browser can retain downloaded private model bytes on that device. Persistent
caching is a local performance feature, not an access-grant mechanism. Logout
clears model caches and browser cache/storage state. The service worker never
caches authentication responses or profile/world-list responses.

## Configuration

### Shared-world grants

`/api/shared-worlds/*` uses the same signed sessions and same-origin write header.
Creating a world establishes its owner. Existing-world reads require ownership or
an explicit user grant; writes must lie in the grant's block-octree prefixes.
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

With Google credentials unset, the server creates an explicitly named local
development profile in the same object store. This is a development mode, not
Google authentication. Configure Google before exposing a multi-user deployment.

## Scale and concurrency

Per-user prefixes avoid reading every user's manifests to answer one user's
catalog request. The current listing still reads all manifests for that owner.
Paginated immutable catalog pages and cached summaries can scale this without a
database.

Concurrent sign-ins may race when updating profile metadata. The current profile
contains no counters or authorization roles, so last-write-wins affects only
profile freshness/first-creation metadata. If profile fields later carry
security-sensitive state, use object-store conditional updates with ETags and
retries rather than relying on this behavior.
