// Package webhook implements the envproxy mutating admission webhook.
package webhook

import (
	"context"
	"encoding/json"
	"net/http"

	"github.com/go-logr/logr"
	corev1 "k8s.io/api/core/v1"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/webhook"
	"sigs.k8s.io/controller-runtime/pkg/webhook/admission"

	"github.com/minivolk/EnvProxy/k8s/injector/internal/config"
)

// Mutator handles pod mutation for envproxy injection.
type Mutator struct {
	cfg    *config.Config
	client client.Client
	log    logr.Logger
}

// NewMutator creates a new Mutator with the given configuration.
func NewMutator(cfg *config.Config, c client.Client, log logr.Logger) *Mutator {
	return &Mutator{
		cfg:    cfg,
		client: c,
		log:    log.WithName("mutator"),
	}
}

// Handler returns an admission.Handler for the webhook server.
func (m *Mutator) Handler() http.Handler {
	return &webhook.Admission{
		Handler: m,
	}
}

// Handle implements admission.Handler.
func (m *Mutator) Handle(ctx context.Context, req admission.Request) admission.Response {
	pod := &corev1.Pod{}
	if err := json.Unmarshal(req.Object.Raw, pod); err != nil {
		m.log.Error(err, "failed to unmarshal pod")
		return admission.Errored(http.StatusBadRequest, err)
	}

	// Check if injection is requested.
	annotations := pod.GetAnnotations()
	if annotations == nil || annotations[config.AnnotationInject] != "true" {
		return admission.Allowed("injection not requested")
	}

	// Idempotency guard: skip if already injected.
	if annotations[config.AnnotationStatus] == config.StatusInjected {
		return admission.Allowed("already injected")
	}

	m.log.Info("injecting envproxy",
		"pod", pod.Name,
		"namespace", req.Namespace,
		"containers", len(pod.Spec.Containers),
	)

	// Mutate the pod.
	if err := m.mutatePod(ctx, pod); err != nil {
		m.log.Error(err, "failed to mutate pod")
		return admission.Errored(http.StatusInternalServerError, err)
	}

	// Marshal the mutated pod.
	marshaledPod, err := json.Marshal(pod)
	if err != nil {
		m.log.Error(err, "failed to marshal mutated pod")
		return admission.Errored(http.StatusInternalServerError, err)
	}

	return admission.PatchResponseFromRaw(req.Object.Raw, marshaledPod)
}
