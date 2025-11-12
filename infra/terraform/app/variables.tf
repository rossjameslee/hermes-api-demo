variable "kubeconfig" {
  description = "Path to kubeconfig"
  type        = string
  default     = "~/.kube/config"
}

variable "kube_context" {
  description = "Kube context name"
  type        = string
  default     = null
}

variable "namespace" {
  description = "Kubernetes namespace"
  type        = string
  default     = "hermes-demo"
}

variable "image_repo" {
  description = "Container image repository (e.g., ghcr.io/you/hermes-api)"
  type        = string
}

variable "image_tag" {
  description = "Image tag"
  type        = string
  default     = "latest"
}

variable "replicas" {
  description = "Deployment replica count"
  type        = number
  default     = 2
}

variable "demo_api_keys" {
  description = "Comma-separated org:key list for DEMO_API_KEYS"
  type        = string
  default     = "demo-org:demo-key"
}

variable "openapi_key" {
  description = "Key required for /openapi.json (optional)"
  type        = string
  default     = null
}

variable "metrics_key" {
  description = "Key required for /metrics (optional)"
  type        = string
  default     = null
}

variable "max_images" {
  description = "Max images per request"
  type        = number
  default     = 6
}

variable "request_max_bytes" {
  description = "Body size limit (bytes)"
  type        = number
  default     = 262144
}

variable "http_timeout_secs" {
  description = "HTTP client total timeout"
  type        = number
  default     = 15
}

variable "http_connect_timeout_secs" {
  description = "HTTP client connect timeout"
  type        = number
  default     = 5
}

variable "redis_enabled" {
  description = "Enable Redis for idempotency"
  type        = bool
  default     = false
}

variable "redis_url" {
  description = "Redis URL (required if redis_enabled)"
  type        = string
  default     = null
}

variable "idempotency_ttl_secs" {
  description = "TTL for idempotency entries"
  type        = number
  default     = 3600
}

variable "ingress_enabled" {
  description = "Create Ingress resource"
  type        = bool
  default     = false
}

variable "ingress_host" {
  description = "Ingress host (e.g., api.example.com)"
  type        = string
  default     = null
}

variable "ingress_tls_secret" {
  description = "TLS secret name for Ingress (optional)"
  type        = string
  default     = null
}
