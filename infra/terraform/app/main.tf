resource "kubernetes_namespace" "ns" {
  metadata { name = var.namespace }
}

resource "kubernetes_secret" "env" {
  metadata {
    name      = "hermes-env"
    namespace = kubernetes_namespace.ns.metadata[0].name
  }
  data = {
    DEMO_API_KEYS               = var.demo_api_keys
    OPENAPI_KEY                 = var.openapi_key != null ? var.openapi_key : ""
    METRICS_KEY                 = var.metrics_key != null ? var.metrics_key : ""
    MAX_IMAGES                  = tostring(var.max_images)
    REQUEST_MAX_BYTES           = tostring(var.request_max_bytes)
    HTTP_TIMEOUT_SECS           = tostring(var.http_timeout_secs)
    HTTP_CONNECT_TIMEOUT_SECS   = tostring(var.http_connect_timeout_secs)
    IDEMPOTENCY_TTL_SECS        = tostring(var.idempotency_ttl_secs)
    REDIS_URL                   = var.redis_enabled && var.redis_url != null ? var.redis_url : ""
  }
  type = "Opaque"
}

resource "kubernetes_deployment" "hermes" {
  metadata {
    name      = "hermes-api"
    namespace = kubernetes_namespace.ns.metadata[0].name
    labels = { app = "hermes-api" }
  }
  spec {
    replicas = var.replicas
    selector { match_labels = { app = "hermes-api" } }
    template {
      metadata { labels = { app = "hermes-api" } }
      spec {
        container {
          name  = "hermes-api"
          image = "${var.image_repo}:${var.image_tag}"
          port { container_port = 8000 }
          env_from { secret_ref { name = kubernetes_secret.env.metadata[0].name } }
          readiness_probe {
            http_get { path = "/health" port = 8000 }
            initial_delay_seconds = 3
            period_seconds        = 5
          }
          liveness_probe {
            http_get { path = "/health" port = 8000 }
            initial_delay_seconds = 10
            period_seconds        = 10
          }
          resources {
            limits = {
              cpu    = "500m"
              memory = "512Mi"
            }
            requests = {
              cpu    = "100m"
              memory = "128Mi"
            }
          }
        }
      }
    }
  }
}

resource "kubernetes_service" "svc" {
  metadata {
    name      = "hermes-api"
    namespace = kubernetes_namespace.ns.metadata[0].name
    labels    = { app = "hermes-api" }
  }
  spec {
    selector = { app = "hermes-api" }
    port {
      name        = "http"
      port        = 80
      target_port = 8000
      protocol    = "TCP"
    }
    type = "ClusterIP"
  }
}

# Optional Ingress
resource "kubernetes_ingress_v1" "http" {
  count = var.ingress_enabled ? 1 : 0
  metadata {
    name      = "hermes-api"
    namespace = kubernetes_namespace.ns.metadata[0].name
    annotations = {
      "kubernetes.io/ingress.class"                 = "nginx"
      "nginx.ingress.kubernetes.io/proxy-body-size" = "256k"
    }
  }
  spec {
    rule {
      host = coalesce(var.ingress_host, "hermes.local")
      http {
        path {
          path      = "/"
          path_type = "Prefix"
          backend {
            service {
              name = kubernetes_service.svc.metadata[0].name
              port { number = 80 }
            }
          }
        }
      }
    }
    dynamic "tls" {
      for_each = var.ingress_tls_secret == null ? [] : [1]
      content {
        secret_name = var.ingress_tls_secret
        hosts       = [coalesce(var.ingress_host, "hermes.local")]
      }
    }
  }
}
