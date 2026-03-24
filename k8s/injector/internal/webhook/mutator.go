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
// 1. Adds volumes (emptyDir for binaries, hostPath for agent socket)
// 2. Adds init container (copies envproxy binaries from the envproxy image)
// 3. Wraps each container's entrypoint with "envproxy run --"
// 4. Adds environment variables and volume mounts to each container
// 5. Marks the pod as injected
func (m *Mutator) mutatePod(ctx context.Context, pod *corev1.Pod) error {
	cacheTTL := m.cfg.DefaultCacheTTL
	if v, ok := pod.Annotations[config.AnnotationCacheTTL]; ok {
		cacheTTL = v
	}

	noPython := pod.Annotations[config.AnnotationNoPython] == "true"
	noJava := pod.Annotations[config.AnnotationNoJava] == "true"

	// Which containers to inject (empty = all).
	targetContainers := parseContainerList(pod.Annotations[config.AnnotationContainers])

	// 1. Add volumes.
	pod.Spec.Volumes = append(pod.Spec.Volumes,
		corev1.Volume{
			Name: config.VolumeName,
			VolumeSource: corev1.VolumeSource{
				EmptyDir: &corev1.EmptyDirVolumeSource{
					Medium: corev1.StorageMediumMemory,
				},
			},
		},
		corev1.Volume{
			Name: config.SocketVolumeName,
			VolumeSource: corev1.VolumeSource{
				HostPath: &corev1.HostPathVolumeSource{
					Path: config.HostSocketPath,
					Type: hostPathTypePtr(corev1.HostPathDirectory),
				},
			},
		},
	)

	// 2. Add init container.
	pod.Spec.InitContainers = append(pod.Spec.InitContainers, m.buildInitContainer())

	// 3. Mutate each target container.
	for i := range pod.Spec.Containers {
		c := &pod.Spec.Containers[i]

		if !shouldInject(c.Name, targetContainers) {
			continue
		}

		if err := m.mutateContainer(ctx, c, pod, cacheTTL, noPython, noJava); err != nil {
			return fmt.Errorf("container %q: %w", c.Name, err)
		}
	}

	// 4. Mark as injected.
	if pod.Annotations == nil {
		pod.Annotations = make(map[string]string)
	}
	pod.Annotations[config.AnnotationStatus] = config.StatusInjected

	return nil
}

// buildInitContainer creates the init container that copies envproxy binaries.
func (m *Mutator) buildInitContainer() corev1.Container {
	return corev1.Container{
		Name:    "envproxy-init",
		Image:   m.cfg.EnvproxyImage,
		Command: []string{"sh", "-c"},
		Args: []string{
			"cp /usr/bin/envproxy /envproxy/envproxy && " +
				"mkdir -p /envproxy/lib /envproxy/python /envproxy/java && " +
				"cp /usr/lib/envproxy/lib/libenvproxy.so /envproxy/lib/libenvproxy.so && " +
				"cp -r /usr/lib/envproxy/python/* /envproxy/python/ && " +
				"cp /usr/lib/envproxy/java/envproxy-agent.jar /envproxy/java/envproxy-agent.jar",
		},
		VolumeMounts: []corev1.VolumeMount{
			{
				Name:      config.VolumeName,
				MountPath: config.MountPath,
			},
		},
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

// mutateContainer wraps a single container's entrypoint with envproxy.
func (m *Mutator) mutateContainer(
	ctx context.Context,
	c *corev1.Container,
	pod *corev1.Pod,
	cacheTTL string,
	noPython, noJava bool,
) error {
	// Discover the original command.
	originalCmd := c.Command
	originalArgs := c.Args

	// If command is not set, we need to discover it from the image.
	// For now, if command is empty, we require it to be set explicitly.
	// Registry lookup will be added in a future version.
	if len(originalCmd) == 0 {
		m.log.Info("container has no explicit command, skipping entrypoint wrapping (set command: in pod spec)",
			"container", c.Name,
		)
		// Still add env vars and volume mounts for LD_PRELOAD to work
		// if the image's entrypoint happens to call getenv.
	}

	// Build the wrapped command: /envproxy/envproxy run -- <original>
	if len(originalCmd) > 0 {
		wrappedArgs := []string{"run", "--"}
		wrappedArgs = append(wrappedArgs, originalCmd...)
		wrappedArgs = append(wrappedArgs, originalArgs...)

		c.Command = []string{config.MountPath + "/envproxy"}
		c.Args = wrappedArgs
	}

	// Add volume mounts.
	c.VolumeMounts = append(c.VolumeMounts,
		corev1.VolumeMount{
			Name:      config.VolumeName,
			MountPath: config.MountPath,
			ReadOnly:  true,
		},
		corev1.VolumeMount{
			Name:      config.SocketVolumeName,
			MountPath: config.SocketMountPath,
			ReadOnly:  true,
		},
	)

	// Add environment variables.
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

// parseContainerList splits a comma-separated container list annotation.
func parseContainerList(s string) map[string]bool {
	if s == "" {
		return nil // nil means "all containers"
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

// shouldInject checks whether a container should be injected.
func shouldInject(name string, targets map[string]bool) bool {
	if targets == nil {
		return true // inject all
	}
	return targets[name]
}

func hostPathTypePtr(t corev1.HostPathType) *corev1.HostPathType {
	return &t
}
