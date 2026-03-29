# Minimum Vault policy for envproxy.
#
# Usage:
#   vault policy write envproxy-policy k8s/examples/vault-policy.hcl
#
#   vault auth enable kubernetes
#   vault write auth/kubernetes/config \
#     kubernetes_host="https://kubernetes.default.svc"
#
#   vault write auth/kubernetes/role/myapp \
#     bound_service_account_names=myapp \
#     bound_service_account_namespaces=default \
#     policies=envproxy-policy \
#     ttl=1h

# Allow reading KV v2 secrets under the myapp/ path.
path "secret/data/myapp/*" {
  capabilities = ["read"]
}

# Allow reading secret metadata (optional, for versioned reads).
path "secret/metadata/myapp/*" {
  capabilities = ["read"]
}
