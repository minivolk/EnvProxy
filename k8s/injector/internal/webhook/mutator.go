package webhook

import (
	"context"
	"fmt"
	"strings"

	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/api/resource"

	"github.com/minivolk/EnvProxy/k8s/injector/internal/config"
)

// mutatePod performs the actual pod mutation:
// 1. Adds emptyDir volume (shared between init, sidecar, and app containers)
// 2. Adds init container (copies envproxy binaries + generates agent config)
// 3. Adds sidecar container (envproxy-agent with Vault backend)
// 4. Wraps each app container's entrypoint with "envproxy run --"
// 5. Adds environment variables and volume mounts to each app container
// 6. Marks the pod as injected
func (m *Mutator) mutatePod(ctx context.Context, pod *corev1.Pod) error {
	cacheTTL := m.cfg.DefaultCacheTTL
	if v, ok := pod.Annotations[config.AnnotationCacheTTL]; ok {
		cacheTTL = v
	}

	noPython := pod.Annotations[config.AnnotationNoPython] == "true"
	noJava := pod.Annotations[config.AnnotationNoJava] == "true"

	targetContainers := parseContainerList(pod.Annotations[config.AnnotationContainers])

	// 1. Add shared emptyDir volume.
	pod.Spec.Volumes = append(pod.Spec.Volumes,
		corev1.Volume{
			Name: config.VolumeName,
			VolumeSource: corev1.VolumeSource{
				EmptyDir: &corev1.EmptyDirVolumeSource{
					Medium: corev1.StorageMediumMemory,
				},
			},
		},
	)

	// 2. Add init container (copies binaries + writes agent config).
	pod.Spec.InitContainers = append(pod.Spec.InitContainers,
		m.buildInitContainer(pod),
	)

	// 3. Add sidecar container (envproxy-agent).
	pod.Spec.Containers = append(pod.Spec.Containers,
		m.buildSidecar(pod),
	)

	// 4. Mutate each target app container.
	for i := range pod.Spec.Containers {
		c := &pod.Spec.Containers[i]

		// Skip the sidecar we just added.
		if c.Name == "envproxy-agent" {
			continue
		}

		if !shouldInject(c.Name, targetContainers) {
			continue
		}

		if err := m.mutateContainer(ctx, c, pod, cacheTTL, noPython, noJava); err != nil {
			return fmt.Errorf("container %q: %w", c.Name, err)
		}
	}

	// 5. Mark as injected.
	if pod.Annotations == nil {
		pod.Annotations = make(map[string]string)
	}
	pod.Annotations[config.AnnotationStatus] = config.StatusInjected

	return nil
}

// buildInitContainer creates the init container that copies envproxy binaries
// and generates the agent config.toml from pod annotations.
func (m *Mutator) buildInitContainer(pod *corev1.Pod) corev1.Container {
	// Build the agent config from pod annotations.
	vaultAddr := pod.Annotations[config.AnnotationVaultAddr]
	vaultRole := pod.Annotations[config.AnnotationVaultRole]
	vaultAuthMethod := pod.Annotations[config.AnnotationVaultAuthMethod]
	if vaultAuthMethod == "" {
		vaultAuthMethod = "kubernetes"
	}
	vaultAuthMount := pod.Annotations[config.AnnotationVaultAuthMount]
	if vaultAuthMount == "" {
		vaultAuthMount = "kubernetes"
	}
	vaultCacheTTL := pod.Annotations[config.AnnotationVaultCacheTTL]
	if vaultCacheTTL == "" {
		vaultCacheTTL = "5m"
	}

	// Determine backend type from annotations.
	backendConfig := ""
	if vaultAddr != "" {
		backendConfig = fmt.Sprintf(`[backend]
type = "vault"
address = "%s"
auth_method = "%s"
auth_mount = "%s"
role = "%s"
cache_ttl = "%s"
`, vaultAddr, vaultAuthMethod, vaultAuthMount, vaultRole, vaultCacheTTL)
	} else {
		// No Vault — use a passthrough "file" backend with empty config.
		// The agent will just serve as a relay for LD_PRELOAD interception.
		backendConfig = `[backend]
type = "file"
path = "/dev/null"
`
	}

	configContent := fmt.Sprintf(`[agent]
socket = "%s"
log_level = "info"

%s`, config.SocketPath, backendConfig)

	return corev1.Container{
		Name:  "envproxy-init",
		Image: m.cfg.EnvproxyImage,
		Command: []string{
			"/usr/bin/envproxy", "init",
			"--target", config.MountPath,
			"--write-config", configContent,
		},
		VolumeMounts: []corev1.VolumeMount{
			{Name: config.VolumeName, MountPath: config.MountPath},
		},
		SecurityContext: secureContext(),
		Resources: corev1.ResourceRequirements{
			Limits: corev1.ResourceList{
				corev1.ResourceCPU:    resource.MustParse("50m"),
				corev1.ResourceMemory: resource.MustParse("32Mi"),
			},
			Requests: corev1.ResourceList{
				corev1.ResourceCPU:    resource.MustParse("10m"),
				corev1.ResourceMemory: resource.MustParse("16Mi"),
			},
		},
	}
}

// buildSidecar creates the envproxy-agent sidecar container.
// Resource limits/requests use config defaults with per-pod annotation overrides.
func (m *Mutator) buildSidecar(pod *corev1.Pod) corev1.Container {
	cpuLimit := annotationOrDefault(pod, config.AnnotationAgentCPULimit, m.cfg.AgentCPULimit)
	memLimit := annotationOrDefault(pod, config.AnnotationAgentMemoryLimit, m.cfg.AgentMemoryLimit)
	cpuReq := annotationOrDefault(pod, config.AnnotationAgentCPURequest, m.cfg.AgentCPURequest)
	memReq := annotationOrDefault(pod, config.AnnotationAgentMemoryRequest, m.cfg.AgentMemoryRequest)

	return corev1.Container{
		Name:    "envproxy-agent",
		Image:   m.cfg.EnvproxyImage,
		Command: []string{"/envproxy/envproxy-agent", "--config", "/envproxy/config.toml"},
		VolumeMounts: []corev1.VolumeMount{
			{Name: config.VolumeName, MountPath: config.MountPath},
		},
		SecurityContext: secureContext(),
		Resources: corev1.ResourceRequirements{
			Limits: corev1.ResourceList{
				corev1.ResourceCPU:    resource.MustParse(cpuLimit),
				corev1.ResourceMemory: resource.MustParse(memLimit),
			},
			Requests: corev1.ResourceList{
				corev1.ResourceCPU:    resource.MustParse(cpuReq),
				corev1.ResourceMemory: resource.MustParse(memReq),
			},
		},
	}
}

// annotationOrDefault returns the pod annotation value if set, otherwise the default.
func annotationOrDefault(pod *corev1.Pod, key, defaultVal string) string {
	if pod.Annotations != nil {
		if v, ok := pod.Annotations[key]; ok && v != "" {
			return v
		}
	}
	return defaultVal
}

// mutateContainer wraps a single container's entrypoint with envproxy.
func (m *Mutator) mutateContainer(
	ctx context.Context,
	c *corev1.Container,
	pod *corev1.Pod,
	cacheTTL string,
	noPython, noJava bool,
) error {
	originalCmd := c.Command
	originalArgs := c.Args

	if len(originalCmd) == 0 {
		m.log.Info("container has no explicit command, skipping entrypoint wrapping",
			"container", c.Name,
		)
	}

	// Wrap command: /envproxy/envproxy run -- <original>
	if len(originalCmd) > 0 {
		wrappedArgs := []string{"run", "--"}
		wrappedArgs = append(wrappedArgs, originalCmd...)
		wrappedArgs = append(wrappedArgs, originalArgs...)

		c.Command = []string{config.MountPath + "/envproxy"}
		c.Args = wrappedArgs
	}

	// Volume mount (shared emptyDir with init + sidecar).
	c.VolumeMounts = append(c.VolumeMounts,
		corev1.VolumeMount{
			Name:      config.VolumeName,
			MountPath: config.MountPath,
			ReadOnly:  true,
		},
	)

	// Environment variables.
	envVars := []corev1.EnvVar{
		{Name: "ENVPROXY_SOCKET", Value: config.SocketPath},
		{Name: "ENVPROXY_LIB", Value: config.MountPath + "/lib/libenvproxy.so"},
		{Name: "ENVPROXY_CACHE_TTL", Value: cacheTTL},
	}

	if !noPython {
		envVars = append(envVars,
			corev1.EnvVar{Name: "ENVPROXY_PYTHON_PATH", Value: config.MountPath + "/python"},
		)
	} else {
		envVars = append(envVars,
			corev1.EnvVar{Name: "ENVPROXY_NO_PYTHON", Value: "1"},
		)
	}

	if !noJava {
		envVars = append(envVars,
			corev1.EnvVar{Name: "ENVPROXY_JAVA_PATH", Value: config.MountPath + "/java"},
		)
	} else {
		envVars = append(envVars,
			corev1.EnvVar{Name: "ENVPROXY_NO_JAVA", Value: "1"},
		)
	}

	c.Env = append(c.Env, envVars...)

	return nil
}

func parseContainerList(s string) map[string]bool {
	if s == "" {
		return nil
	}
	result := make(map[string]bool)
	for _, name := range strings.Split(s, ",") {
		name = strings.TrimSpace(name)
		if name != "" {
			result[name] = true
		}
	}
	return result
}

func shouldInject(name string, targets map[string]bool) bool {
	if targets == nil {
		return true
	}
	return targets[name]
}

// secureContext returns a hardened SecurityContext for injected containers:
// non-root, read-only root filesystem, no privilege escalation, all capabilities dropped.
func secureContext() *corev1.SecurityContext {
	return &corev1.SecurityContext{
		RunAsNonRoot:             boolPtr(true),
		ReadOnlyRootFilesystem:   boolPtr(true),
		AllowPrivilegeEscalation: boolPtr(false),
		Capabilities: &corev1.Capabilities{
			Drop: []corev1.Capability{"ALL"},
		},
	}
}

func boolPtr(b bool) *bool {
	return &b
}
