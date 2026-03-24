// Package tls generates self-signed TLS certificates for the webhook server
// and patches the MutatingWebhookConfiguration with the CA bundle.
//
// This eliminates the need for cert-manager in development/testing environments.
// In production, use cert-manager or provide your own TLS secret.
package tls

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/pem"
	"fmt"
	"math/big"
	"os"
	"path/filepath"
	"time"

	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/client-go/kubernetes"
	"k8s.io/client-go/rest"
)

// Options configures self-signed TLS certificate generation.
type Options struct {
	// CertDir is the directory where tls.crt and tls.key will be written.
	CertDir string

	// ServiceName is the webhook service name (e.g., "envproxy-injector").
	ServiceName string

	// Namespace is the namespace where the webhook service runs.
	Namespace string

	// WebhookName is the MutatingWebhookConfiguration name to patch with caBundle.
	WebhookName string
}

// GenerateAndPatch generates a self-signed CA + server certificate,
// writes them to disk, and patches the MutatingWebhookConfiguration
// with the CA bundle so the API server trusts our webhook.
func GenerateAndPatch(ctx context.Context, opts Options) error {
	// Generate CA key pair.
	caKey, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		return fmt.Errorf("generate CA key: %w", err)
	}

	caTemplate := &x509.Certificate{
		SerialNumber: big.NewInt(1),
		Subject: pkix.Name{
			CommonName:   "envproxy-injector-ca",
			Organization: []string{"envproxy"},
		},
		NotBefore:             time.Now().Add(-1 * time.Hour),
		NotAfter:              time.Now().Add(10 * 365 * 24 * time.Hour), // 10 years
		KeyUsage:              x509.KeyUsageCertSign | x509.KeyUsageCRLSign,
		BasicConstraintsValid: true,
		IsCA:                  true,
	}

	caCertDER, err := x509.CreateCertificate(rand.Reader, caTemplate, caTemplate, &caKey.PublicKey, caKey)
	if err != nil {
		return fmt.Errorf("create CA cert: %w", err)
	}

	caCert, err := x509.ParseCertificate(caCertDER)
	if err != nil {
		return fmt.Errorf("parse CA cert: %w", err)
	}

	caCertPEM := pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: caCertDER})

	// Generate server key pair.
	serverKey, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		return fmt.Errorf("generate server key: %w", err)
	}

	dnsNames := []string{
		opts.ServiceName,
		fmt.Sprintf("%s.%s", opts.ServiceName, opts.Namespace),
		fmt.Sprintf("%s.%s.svc", opts.ServiceName, opts.Namespace),
		fmt.Sprintf("%s.%s.svc.cluster.local", opts.ServiceName, opts.Namespace),
	}

	serverTemplate := &x509.Certificate{
		SerialNumber: big.NewInt(2),
		Subject: pkix.Name{
			CommonName:   opts.ServiceName,
			Organization: []string{"envproxy"},
		},
		DNSNames:  dnsNames,
		NotBefore: time.Now().Add(-1 * time.Hour),
		NotAfter:  time.Now().Add(10 * 365 * 24 * time.Hour),
		KeyUsage:  x509.KeyUsageDigitalSignature | x509.KeyUsageKeyEncipherment,
		ExtKeyUsage: []x509.ExtKeyUsage{
			x509.ExtKeyUsageServerAuth,
		},
	}

	serverCertDER, err := x509.CreateCertificate(rand.Reader, serverTemplate, caCert, &serverKey.PublicKey, caKey)
	if err != nil {
		return fmt.Errorf("create server cert: %w", err)
	}

	serverCertPEM := pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: serverCertDER})

	serverKeyDER, err := x509.MarshalECPrivateKey(serverKey)
	if err != nil {
		return fmt.Errorf("marshal server key: %w", err)
	}
	serverKeyPEM := pem.EncodeToMemory(&pem.Block{Type: "EC PRIVATE KEY", Bytes: serverKeyDER})

	// Write certs to disk.
	if err := os.MkdirAll(opts.CertDir, 0o700); err != nil {
		return fmt.Errorf("create cert dir: %w", err)
	}

	if err := os.WriteFile(filepath.Join(opts.CertDir, "tls.crt"), serverCertPEM, 0o600); err != nil {
		return fmt.Errorf("write tls.crt: %w", err)
	}
	if err := os.WriteFile(filepath.Join(opts.CertDir, "tls.key"), serverKeyPEM, 0o600); err != nil {
		return fmt.Errorf("write tls.key: %w", err)
	}

	// Patch the MutatingWebhookConfiguration with the CA bundle.
	if opts.WebhookName != "" {
		if err := patchWebhookCABundle(ctx, opts.WebhookName, caCertPEM); err != nil {
			return fmt.Errorf("patch webhook caBundle: %w", err)
		}
	}

	return nil
}

// patchWebhookCABundle patches the MutatingWebhookConfiguration to set
// the caBundle on all webhooks.
func patchWebhookCABundle(ctx context.Context, webhookName string, caBundle []byte) error {
	cfg, err := rest.InClusterConfig()
	if err != nil {
		return fmt.Errorf("in-cluster config: %w", err)
	}

	clientset, err := kubernetes.NewForConfig(cfg)
	if err != nil {
		return fmt.Errorf("create clientset: %w", err)
	}

	// Get the current webhook configuration.
	whc, err := clientset.AdmissionregistrationV1().MutatingWebhookConfigurations().Get(
		ctx, webhookName, metav1.GetOptions{},
	)
	if err != nil {
		return fmt.Errorf("get webhook %q: %w", webhookName, err)
	}

	// Set caBundle on all webhooks.
	for i := range whc.Webhooks {
		whc.Webhooks[i].ClientConfig.CABundle = caBundle
	}

	// Update the full webhook configuration with the new caBundle.
	_, err = clientset.AdmissionregistrationV1().MutatingWebhookConfigurations().Update(
		ctx, whc, metav1.UpdateOptions{},
	)
	if err != nil {
		return fmt.Errorf("update webhook %q: %w", webhookName, err)
	}

	return nil
}
