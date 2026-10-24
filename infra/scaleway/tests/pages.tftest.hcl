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
  image                = "ghcr.io/ngrislain/ergo:test"
  google_client_id     = "synthetic.apps.googleusercontent.com"
  google_client_secret = "synthetic-oauth-secret"
  session_secret       = "synthetic-session-secret-at-least-32-characters"
  s3_access_key_id     = "synthetic-access-key"
  s3_secret_access_key = "synthetic-secret-key"
}

run "www_is_a_separate_github_pages_cname" {
  command = plan
  assert {
    condition     = scaleway_domain_record.pages[0].name == "www" && scaleway_domain_record.pages[0].type == "CNAME" && scaleway_domain_record.pages[0].data == "ngrislain.github.io."
    error_message = "www must point to the GitHub Pages account, not the authenticated service or repository URL."
  }
  assert {
    condition     = scaleway_domain_record.api[0].name == "" && scaleway_domain_record.api[0].type == "ALIAS" && scaleway_container.api.environment_variables["ERGO_PUBLIC_URL"] == "https://ergosandbox.com"
    error_message = "The API and Google OAuth origin must remain on the apex domain."
  }
}

run "external_dns_omits_www_record" {
  command = plan
  variables { manage_dns = false }
  assert {
    condition     = length(scaleway_domain_record.pages) == 0
    error_message = "Externally managed DNS must not create the www record."
  }
}
