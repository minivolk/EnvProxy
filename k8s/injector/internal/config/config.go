// Package config holds the injector configuration.
package config

// Config holds the webhook configuration values.
type Config struct {
	// EnvproxyImage is the container image used for the init container
	// that copies envproxy binaries into the shared volume.
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
	// If empty, all containers are injected.
	AnnotationContainers = "envproxy.dev/containers"

	// AnnotationNoPython disables Python os.environ patching when set to "true".
	AnnotationNoPython = "envproxy.dev/no-python"

	// AnnotationNoJava disables Java System.getenv() patching when set to "true".
	AnnotationNoJava = "envproxy.dev/no-java"

	// StatusInjected is the value set on AnnotationStatus after injection.
	StatusInjected = "injected"

	// VolumeName is the name of the shared emptyDir volume for envproxy binaries.
	VolumeName = "envproxy-bin"

	// SocketVolumeName is the name of the hostPath volume for the agent socket.
	SocketVolumeName = "envproxy-socket"

	// MountPath is where envproxy binaries are mounted in the app container.
	MountPath = "/envproxy"

	// SocketMountPath is where the agent socket is mounted.
	SocketMountPath = "/var/run/envproxy"

	// SocketPath is the full path to the agent socket inside the container.
	SocketPath = "/var/run/envproxy/agent.sock"

	// HostSocketPath is the host path where the DaemonSet agent socket lives.
	HostSocketPath = "/var/run/envproxy"
)
