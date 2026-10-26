locals {
  public_url  = "https://${var.domain_name}"
  s3_endpoint = "https://s3.${var.bucket_region}.scw.cloud"
  dns_target  = "${trimprefix(scaleway_container.api.public_endpoint, "https://")}."
  sandbox_environment = var.common_world_id == "" ? {} : {
    ERGO_COMMON_WORLD_ID = var.common_world_id
    ERGO_WORLD_PREFIX    = var.world_prefix
  }
}

# This is an existing bucket, deliberately not a managed resource.
data "scaleway_object_bucket" "ergo" {
  name       = var.bucket_name
  region     = var.bucket_region
  project_id = var.project_id
}

resource "scaleway_container_namespace" "ergo" {
  name        = "ergo"
  description = "Ergo sandbox API and web application"
  tags        = ["ergo", "opentofu"]
}

resource "scaleway_container" "api" {
  name                   = "ergo-api"
  namespace_id           = scaleway_container_namespace.ergo.id
  image                  = var.image
  port                   = 8080
  privacy                = "public"
  protocol               = "http1"
  https_connections_only = true
  cpu_limit              = 1000
  # Scaleway stores whole decimal MB; round 2 GiB up to avoid perpetual drift.
  memory_limit_bytes = 2148 * 1000 * 1000
  min_scale          = 0
  max_scale          = var.max_scale
  timeout            = 300

  scaling_option {
    concurrent_requests_threshold = 10
  }

  environment_variables = merge({ ERGO_GROUND_SOURCE = "osm" }, var.extra_environment_variables, local.sandbox_environment, {
    ERGO_BIND                   = "0.0.0.0:8080"
    ERGO_PUBLIC_URL             = local.public_url
    S3_BUCKET                   = data.scaleway_object_bucket.ergo.name
    S3_REGION                   = var.bucket_region
    S3_ENDPOINT                 = local.s3_endpoint
    S3_PREFIX                   = var.s3_prefix
    GOOGLE_CLIENT_ID            = var.google_client_id
    ERGO_MAP_USER_AGENT         = "Ergo (https://${var.domain_name})"
    ERGO_EDIT_BRICKS_PER_PERIOD = tostring(var.free_bricks_per_period)
    ERGO_EDIT_PERIOD_SECONDS    = tostring(var.edit_period_seconds)
  })

  secret_environment_variables = {
    AWS_ACCESS_KEY_ID     = var.s3_access_key_id
    AWS_SECRET_ACCESS_KEY = var.s3_secret_access_key
    GOOGLE_CLIENT_SECRET  = var.google_client_secret
    ERGO_SESSION_SECRET   = var.session_secret
  }

  liveness_probe {
    http {
      path = "/api/config"
    }
    interval          = "30s"
    timeout           = "5s"
    failure_threshold = 3
  }
  startup_probe {
    http {
      path = "/api/config"
    }
    interval          = "5s"
    timeout           = "5s"
    failure_threshold = 24
  }
}

# ALIAS, rather than CNAME, supports the ergosandbox.com zone apex.
resource "scaleway_domain_record" "api" {
  count    = var.manage_dns ? 1 : 0
  dns_zone = var.domain_name
  name     = ""
  type     = "ALIAS"
  data     = local.dns_target
  ttl      = 300
}

# On first deployment, create DNS with bind_custom_domain=false; enable this
# after public resolvers see the ALIAS, so Scaleway's TLS challenge can succeed.
resource "scaleway_container_domain" "api" {
  count        = var.bind_custom_domain ? 1 : 0
  container_id = scaleway_container.api.id
  hostname     = var.domain_name
  depends_on   = [scaleway_domain_record.api]
}
