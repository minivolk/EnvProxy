// Package main starts the envproxy mutating admission webhook server.
package main

import (
	"context"
	"flag"
	"os"

	"github.com/minivolk/EnvProxy/k8s/injector/internal/config"
	selfsignedtls "github.com/minivolk/EnvProxy/k8s/injector/internal/tls"
	"github.com/minivolk/EnvProxy/k8s/injector/internal/webhook"

	"k8s.io/apimachinery/pkg/runtime"
	clientgoscheme "k8s.io/client-go/kubernetes/scheme"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/healthz"
	"sigs.k8s.io/controller-runtime/pkg/log/zap"
	ctrlwebhook "sigs.k8s.io/controller-runtime/pkg/webhook"
)

func main() {
	var envproxyImage string
	var defaultCacheTTL string
	var port int
	var selfSignedTLS bool
	var webhookName string
	var serviceName string
	var namespace string
	var certDir string
	var agentCPULimit string
	var agentMemoryLimit string
	var agentCPURequest string
	var agentMemoryRequest string

	flag.StringVar(&envproxyImage, "envproxy-image", "ghcr.io/minivolk/envproxy:latest", "envproxy container image for init container")
	flag.StringVar(&defaultCacheTTL, "default-cache-ttl", "30", "default cache TTL in seconds for Python/Java")
	flag.IntVar(&port, "port", 9443, "webhook server port")
	flag.BoolVar(&selfSignedTLS, "self-signed-tls", false, "generate self-signed TLS certs at startup and patch webhook caBundle")
	flag.StringVar(&webhookName, "webhook-name", "envproxy-injector", "MutatingWebhookConfiguration name to patch with caBundle")
	flag.StringVar(&serviceName, "service-name", "envproxy-injector", "webhook service name for TLS cert DNS names")
	flag.StringVar(&namespace, "namespace", "", "namespace of the webhook service (auto-detected from downward API if empty)")
	flag.StringVar(&certDir, "cert-dir", "/tmp/k8s-webhook-server/serving-certs", "directory for TLS certs")
	flag.StringVar(&agentCPULimit, "agent-cpu-limit", "50m", "default CPU limit for sidecar agent")
	flag.StringVar(&agentMemoryLimit, "agent-memory-limit", "64Mi", "default memory limit for sidecar agent")
	flag.StringVar(&agentCPURequest, "agent-cpu-request", "10m", "default CPU request for sidecar agent")
	flag.StringVar(&agentMemoryRequest, "agent-memory-request", "32Mi", "default memory request for sidecar agent")
	flag.Parse()

	ctrl.SetLogger(zap.New(zap.UseDevMode(true)))
	log := ctrl.Log.WithName("envproxy-injector")

	// Auto-detect namespace from downward API or environment.
	if namespace == "" {
		if ns := os.Getenv("POD_NAMESPACE"); ns != "" {
			namespace = ns
		} else {
			// Try reading from service account mount.
			if data, err := os.ReadFile("/var/run/secrets/kubernetes.io/serviceaccount/namespace"); err == nil {
				namespace = string(data)
			} else {
				namespace = "default"
			}
		}
	}

	// Generate self-signed TLS certs if requested.
	if selfSignedTLS {
		log.Info("generating self-signed TLS certificates",
			"cert-dir", certDir,
			"service", serviceName,
			"namespace", namespace,
			"webhook", webhookName,
		)

		err := selfsignedtls.GenerateAndPatch(context.Background(), selfsignedtls.Options{
			CertDir:     certDir,
			ServiceName: serviceName,
			Namespace:   namespace,
			WebhookName: webhookName,
		})
		if err != nil {
			log.Error(err, "failed to generate self-signed TLS certs")
			os.Exit(1)
		}

		log.Info("self-signed TLS certificates generated and webhook patched")
	}

	scheme := runtime.NewScheme()
	_ = clientgoscheme.AddToScheme(scheme)

	mgr, err := ctrl.NewManager(ctrl.GetConfigOrDie(), ctrl.Options{
		Scheme:                 scheme,
		HealthProbeBindAddress: ":8081",
		WebhookServer: ctrlwebhook.NewServer(ctrlwebhook.Options{
			Port:    port,
			CertDir: certDir,
		}),
	})
	if err != nil {
		log.Error(err, "unable to create manager")
		os.Exit(1)
	}

	if err := mgr.AddHealthzCheck("healthz", healthz.Ping); err != nil {
		log.Error(err, "unable to set up health check")
		os.Exit(1)
	}
	if err := mgr.AddReadyzCheck("readyz", healthz.Ping); err != nil {
		log.Error(err, "unable to set up ready check")
		os.Exit(1)
	}

	cfg := &config.Config{
		EnvproxyImage:      envproxyImage,
		DefaultCacheTTL:    defaultCacheTTL,
		AgentCPULimit:      agentCPULimit,
		AgentMemoryLimit:   agentMemoryLimit,
		AgentCPURequest:    agentCPURequest,
		AgentMemoryRequest: agentMemoryRequest,
	}

	mutator := webhook.NewMutator(cfg, mgr.GetClient(), log)

	mgr.GetWebhookServer().Register("/mutate", mutator.Handler())

	log.Info("starting envproxy-injector",
		"port", port,
		"envproxy-image", envproxyImage,
		"self-signed-tls", selfSignedTLS,
	)

	if err := mgr.Start(ctrl.SetupSignalHandler()); err != nil {
		log.Error(err, "manager exited with error")
		os.Exit(1)
	}
}
