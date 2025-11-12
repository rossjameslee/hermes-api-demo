output "service_name" {
  description = "Kubernetes Service name"
  value       = kubernetes_service.svc.metadata[0].name
}

output "namespace" {
  description = "Namespace"
  value       = kubernetes_namespace.ns.metadata[0].name
}

output "ingress_hosts" {
  description = "Ingress hosts"
  value       = var.ingress_enabled ? [for r in kubernetes_ingress_v1.http[0].spec[0].rule : r.host] : []
}

