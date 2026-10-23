variable "project_id" {
  description = "Scaleway project UUID; null uses SCW_DEFAULT_PROJECT_ID/provider configuration."
  type        = string
  default     = null
}

variable "region" {
  description = "Serverless Container region."
  type        = string
  default     = "fr-par"
}

variable "bucket_region" {
  description = "Region of the existing ergo bucket."
  type        = string
  default     = "fr-par"
}

variable "bucket_name" {
  description = "Existing Object Storage bucket; it is looked up, never created or destroyed."
  type        = string
  default     = "ergo"
}

variable "s3_prefix" {
  description = "Existing application object prefix inside the bucket."
  type        = string
  default     = "ergo"
}

variable "image" {
  description = "Published linux/amd64 Ergo image, using an immutable tag or digest; private Scaleway images must be accessible to the container project."
  type        = string
  validation {
    condition     = length(trimspace(var.image)) > 0 && !can(regex("\\s|://", var.image))
    error_message = "Supply a published OCI image reference without a URL scheme or whitespace."
  }
}

variable "domain_name" {
  description = "Registered apex domain; DNS must be delegated to Scaleway when manage_dns=true."
  type        = string
  default     = "ergosandbox.com"
  validation {
    condition     = can(regex("^[a-z0-9]([a-z0-9.-]*[a-z0-9])?\\.[a-z]{2,}$", var.domain_name))
    error_message = "Supply a lowercase domain without a scheme, path, or trailing dot."
  }
}

variable "manage_dns" {
  description = "Manage the apex ALIAS in Scaleway DNS; false means configure ALIAS/flattening at the external DNS provider."
  type        = bool
  default     = true
}

variable "bind_custom_domain" {
  description = "Create the custom domain/TLS binding after DNS has propagated."
  type        = bool
  default     = true
}

variable "max_scale" {
  description = "Maximum simultaneous instances; the minimum is always zero."
  type        = number
  default     = 1
  validation {
    condition     = var.max_scale >= 1 && var.max_scale <= 200 && floor(var.max_scale) == var.max_scale
    error_message = "max_scale must be an integer from 1 to 200, within your Scaleway quota."
  }
}

variable "s3_access_key_id" {
  description = "Application S3 access key with read/list/write/delete access to the existing bucket."
  type        = string
  sensitive   = true
  validation {
    condition     = length(trimspace(var.s3_access_key_id)) > 0
    error_message = "An application S3 access key is required."
  }
}

variable "s3_secret_access_key" {
  description = "Application S3 secret key, injected as a container secret environment variable."
  type        = string
  sensitive   = true
  validation {
    condition     = length(trimspace(var.s3_secret_access_key)) > 0
    error_message = "An application S3 secret key is required."
  }
}

variable "google_client_id" {
  description = "Google OAuth web-client ID for https://ergosandbox.com. Production does not use the shared development account."
  type        = string
  validation {
    condition     = length(trimspace(var.google_client_id)) > 0
    error_message = "Configure the Google OAuth client before deploying the public application."
  }
}

variable "google_client_secret" {
  description = "Google OAuth web-client secret."
  type        = string
  sensitive   = true
  validation {
    condition     = length(trimspace(var.google_client_secret)) > 0
    error_message = "The Google OAuth client secret is required."
  }
}

variable "session_secret" {
  description = "Stable session/delegation signing secret shared across cold starts and instances."
  type        = string
  sensitive   = true
  validation {
    condition     = length(var.session_secret) >= 32
    error_message = "Use a persistent random session secret of at least 32 characters."
  }
}

variable "extra_environment_variables" {
  description = "Additional non-secret server settings, such as ERGO_GROUND_SOURCE or ERGO_CONNECTION_MEASUREMENTS."
  type        = map(string)
  default     = {}
  validation {
    condition = alltrue([for key in keys(var.extra_environment_variables) :
      key != "PORT" && !startswith(key, "SCW_") && !contains([
        "ERGO_BIND", "ERGO_PUBLIC_URL", "S3_BUCKET", "S3_REGION", "S3_ENDPOINT", "S3_PREFIX",
        "GOOGLE_CLIENT_ID", "GOOGLE_CLIENT_SECRET", "ERGO_SESSION_SECRET",
        "AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "ERGO_EDIT_BRICKS_PER_PERIOD", "ERGO_EDIT_PERIOD_SECONDS"
      ], key)
    ])
    error_message = "Use the dedicated variables for storage/authentication/origin settings; PORT and SCW_* are platform-reserved."
  }
}

variable "free_bricks_per_period" {
  description = "Free per-user brick-change token-bucket capacity and refill amount. Paid entitlements bypass pacing."
  type        = number
  default     = 5000
  validation {
    condition     = var.free_bricks_per_period >= 1 && var.free_bricks_per_period <= 1000000 && floor(var.free_bricks_per_period) == var.free_bricks_per_period
    error_message = "The free pacing amount must be an integer from 1 to 1000000."
  }
}

variable "edit_period_seconds" {
  description = "Seconds to refill a free user's full brick-change allowance."
  type        = number
  default     = 86400
  validation {
    condition     = var.edit_period_seconds >= 1 && var.edit_period_seconds <= 31536000 && floor(var.edit_period_seconds) == var.edit_period_seconds
    error_message = "The pacing refill period must be 1..31536000 seconds."
  }
}
