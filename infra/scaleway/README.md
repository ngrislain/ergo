# Ergo on Scaleway

OpenTofu configuration for the API/web container at `https://ergosandbox.com`,
scaling from zero to one instance by default and using the existing `ergo` bucket.

Follow the [deployment guide](../../docs/scaleway-deployment.md) for image builds,
Google SSO, secrets, DNS/TLS bootstrap, and validation. Start from
[`terraform.tfvars.example`](terraform.tfvars.example).
