variable "github_pages_host" {
  description = "GitHub Pages account hostname for the public www site, without a trailing dot."
  type        = string
  default     = "ngrislain.github.io"
  validation {
    condition     = can(regex("^[a-z0-9-]+\\.github\\.io$", var.github_pages_host))
    error_message = "Supply a GitHub Pages account hostname such as ngrislain.github.io, without a scheme or trailing dot."
  }
}

# The apex remains the authenticated API/web service; www is the static guide.
resource "scaleway_domain_record" "pages" {
  count    = var.manage_dns ? 1 : 0
  dns_zone = var.domain_name
  name     = "www"
  type     = "CNAME"
  data     = "${var.github_pages_host}."
  ttl      = 300
}

output "site_url" {
  value = "https://www.${var.domain_name}"
}
