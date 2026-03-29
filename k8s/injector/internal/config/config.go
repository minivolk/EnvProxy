// Package config holds the injector configuration.
package config

// Config holds the webhook configuration values.
type Config struct {
	// EnvproxyImage is the container image used for the init container
	// and sidecar agent.
	EnvproxyImage string

	// DefaultCacheTTL is the default cache TTL (seconds) for Python/Java
	// env var caching. Can be overridden per-pod with annotations.
	DefaultCacheTTL string
}

// Annotation keys used by the injector.
const (
	// AnnotationInject triggers injection when set to "true".
	AnnotationInject = "envproxy.dev/inject"

	// AnnotationStatus is set to "injected" after mutation (idempotency guard).
	AnnotationStatus = "envproxy.dev/status"

	// AnnotationCacheTTL overrides the default cache TTL for this pod.
	AnnotationCacheTTL = "envproxy.dev/cache-ttl"

	// AnnotationContainers limits injection to specific containers (comma-separated).
	AnnotationContainers = "envproxy.dev/containers"

	// AnnotationNoPython disables Python os.environ patching when set to "true".
	AnnotationNoPython = "envproxy.dev/no-python"

	// AnnotationNoJava disables Java System.getenv() patching when set to "true".
	AnnotationNoJava = "envproxy.dev/no-java"

	// Vault-specific annotations.
	AnnotationVaultAddr       = "envproxy.dev/vault-addr"
	AnnotationVaultRole       = "envproxy.dev/vault-role"
	AnnotationVaultAuthMethod = "envproxy.dev/vault-auth-method"
	AnnotationVaultAuthMount  = "envproxy.dev/vault-auth-mount"
	AnnotationVaultCACert     = "envproxy.dev/vault-ca-cert"
	AnnotationVaultSkipVerify = "envproxy.dev/vault-tls-skip-verify"
	AnnotationVaultCacheTTL   = "envproxy.dev/vault-cache-ttl"

	// StatusInjected is the value set on AnnotationStatus after injection.
	StatusInjected = "injected"

	// VolumeName is the name of the shared emptyDir volume.
	VolumeName = "envproxy-bin"

	// MountPath is where envproxy binaries are mounted.
	MountPath = "/envproxy"

	// SocketPath is the agent socket inside the shared emptyDir.
	SocketPath = "/envproxy/agent.sock"
)
