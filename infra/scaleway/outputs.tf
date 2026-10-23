output "app_url" {
  value = local.public_url
}

output "container_id" {
  value = scaleway_container.api.id
}

output "container_endpoint" {
  value = scaleway_container.api.public_endpoint
}

output "dns_alias_target" {
  value = local.dns_target
}

output "bucket_name" {
  value = data.scaleway_object_bucket.ergo.name
}

output "google_redirect_uri" {
  value = "${local.public_url}/auth/google/callback"
}
