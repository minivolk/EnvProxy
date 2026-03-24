// Package registry provides image config lookup for discovering container
// ENTRYPOINT and CMD when they are not specified in the pod spec.
package registry

import (
	"context"
	"fmt"
	"sync"

	"github.com/google/go-containerregistry/pkg/authn"
	"github.com/google/go-containerregistry/pkg/authn/k8schain"
	"github.com/google/go-containerregistry/pkg/name"
	"github.com/google/go-containerregistry/pkg/v1/remote"

	corev1 "k8s.io/api/core/v1"
	"k8s.io/client-go/kubernetes"
)

// ImageConfig holds the ENTRYPOINT and CMD from a container image.
type ImageConfig struct {
	Entrypoint []string
	Cmd        []string
}

// Registry resolves image configs from container registries.
type Registry struct {
	client kubernetes.Interface
	cache  sync.Map // map[string]*ImageConfig
}

// New creates a new Registry with the given Kubernetes client.
func New(client kubernetes.Interface) *Registry {
	return &Registry{client: client}
}

// GetImageConfig returns the ENTRYPOINT and CMD for a container image.
// Results are cached by image reference (except for "latest" tags and PullAlways).
func (r *Registry) GetImageConfig(
	ctx context.Context,
	container *corev1.Container,
	pod *corev1.Pod,
) (*ImageConfig, error) {
	ref := container.Image

	// Check cache (skip for latest/PullAlways).
	if canCache(container) {
		if cached, ok := r.cache.Load(ref); ok {
			return cached.(*ImageConfig), nil
		}
	}

	// Build auth keychain from pod's imagePullSecrets + cloud provider creds.
	kc, err := k8schain.New(ctx, r.client, k8schain.Options{
		Namespace:          pod.Namespace,
		ServiceAccountName: pod.Spec.ServiceAccountName,
		ImagePullSecrets:   imagePullSecretNames(pod),
	})
	if err != nil {
		return nil, fmt.Errorf("k8schain auth: %w", err)
	}

	keychain := authn.NewMultiKeychain(kc, authn.DefaultKeychain)

	parsedRef, err := name.ParseReference(ref)
	if err != nil {
		return nil, fmt.Errorf("parse image ref %q: %w", ref, err)
	}

	desc, err := remote.Get(parsedRef, remote.WithAuthFromKeychain(keychain), remote.WithContext(ctx))
	if err != nil {
		return nil, fmt.Errorf("fetch image %q: %w", ref, err)
	}

	img, err := desc.Image()
	if err != nil {
		return nil, fmt.Errorf("image %q: %w", ref, err)
	}

	configFile, err := img.ConfigFile()
	if err != nil {
		return nil, fmt.Errorf("config for %q: %w", ref, err)
	}

	config := &ImageConfig{
		Entrypoint: configFile.Config.Entrypoint,
		Cmd:        configFile.Config.Cmd,
	}

	// Cache if allowed.
	if canCache(container) {
		r.cache.Store(ref, config)
	}

	return config, nil
}

func canCache(c *corev1.Container) bool {
	if c.ImagePullPolicy == corev1.PullAlways {
		return false
	}
	ref, err := name.ParseReference(c.Image)
	if err != nil {
		return false
	}
	return ref.Identifier() != "latest"
}

func imagePullSecretNames(pod *corev1.Pod) []string {
	names := make([]string, 0, len(pod.Spec.ImagePullSecrets))
	for _, s := range pod.Spec.ImagePullSecrets {
		names = append(names, s.Name)
	}
	return names
}
