mock_provider "scaleway" {
  mock_data "scaleway_object_bucket" {
    defaults = {
      id       = "fr-par/ergo"
      endpoint = "https://ergo.s3.fr-par.scw.cloud"
    }
  }
  mock_resource "scaleway_container_namespace" {
    defaults = { id = "fr-par/11111111-1111-4111-8111-111111111111" }
  }
  mock_resource "scaleway_container" {
    defaults = {
      id              = "fr-par/22222222-2222-4222-8222-222222222222"
      public_endpoint = "https://ergo-api.example.functions.fnc.fr-par.scw.cloud"
    }
  }
}

variables {
  project_id           = "11111111-1111-4111-8111-111111111111"
  image                = "rg.fr-par.scw.cloud/test/ergo:test"
  google_client_id     = "synthetic.apps.googleusercontent.com"
  google_client_secret = "synthetic-oauth-secret"
  session_secret       = "synthetic-session-secret-at-least-32-characters"
  s3_access_key_id     = "synthetic-access-key"
  s3_secret_access_key = "synthetic-secret-key"
  common_world_id      = "33333333-3333-4333-8333-333333333333"
}

run "scale_zero_existing_bucket_and_apex_domain" {
  command = apply
  assert {
    condition     = scaleway_container.api.environment_variables["ERGO_COMMON_WORLD_ID"] == var.common_world_id && scaleway_container.api.environment_variables["ERGO_WORLD_PREFIX"] == "world"
    error_message = "The hosted container must target the single migrated world namespace."
  }
  assert {
    condition     = scaleway_container.api.min_scale == 0 && scaleway_container.api.max_scale == 1
    error_message = "The default deployment must scale between zero and one instance."
  }
  assert {
    condition     = scaleway_container.api.memory_limit_bytes >= 2 * 1024 * 1024 * 1024 && scaleway_container.api.memory_limit_bytes % 1000000 == 0
    error_message = "Memory must provide at least 2 GiB in whole decimal MB to match the API's precision."
  }
  assert {
    condition     = scaleway_container.api.environment_variables["S3_BUCKET"] == "ergo" && scaleway_container.api.environment_variables["S3_ENDPOINT"] == "https://s3.fr-par.scw.cloud"
    error_message = "The API must use the existing ergo bucket and regional path-style S3 endpoint."
  }
  assert {
    condition     = scaleway_container.api.environment_variables["ERGO_PUBLIC_URL"] == "https://ergosandbox.com" && scaleway_container_domain.api[0].hostname == "ergosandbox.com"
    error_message = "The public origin and TLS binding must agree on ergosandbox.com."
  }
  assert {
    condition     = scaleway_domain_record.api[0].type == "ALIAS" && scaleway_domain_record.api[0].name == "" && endswith(scaleway_domain_record.api[0].data, ".")
    error_message = "The zone apex needs an ALIAS with an absolute DNS target."
  }
  assert {
    condition     = scaleway_container.api.secret_environment_variables["ERGO_SESSION_SECRET"] == var.session_secret && !contains(keys(scaleway_container.api.environment_variables), "ERGO_SESSION_SECRET")
    error_message = "The persistent signing key must be a secret environment variable."
  }
}

run "external_dns_bootstrap" {
  command = plan
  variables {
    manage_dns         = false
    bind_custom_domain = false
  }
  assert {
    condition     = length(scaleway_domain_record.api) == 0 && length(scaleway_container_domain.api) == 0
    error_message = "External DNS bootstrap must defer the domain binding and managed DNS."
  }
}

run "reject_unstable_signing_secret" {
  command = plan
  variables { session_secret = "short" }
  expect_failures = [var.session_secret]
}

run "reject_public_development_identity" {
  command = plan
  variables { google_client_id = "" }
  expect_failures = [var.google_client_id]
}

run "reject_origin_override" {
  command = plan
  variables { extra_environment_variables = { ERGO_PUBLIC_URL = "http://localhost:3000" } }
  expect_failures = [var.extra_environment_variables]
}
