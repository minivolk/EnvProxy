{{/*
Expand the name of the chart.
*/}}
{{- define "envproxy.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "envproxy.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Chart label values.
*/}}
{{- define "envproxy.labels" -}}
helm.sh/chart: {{ include "envproxy.chart" . }}
{{ include "envproxy.selectorLabels" . }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{- define "envproxy.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "envproxy.selectorLabels" -}}
app.kubernetes.io/name: {{ include "envproxy.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
ServiceAccount name.
*/}}
{{- define "envproxy.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "envproxy.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Injector component names.
*/}}
{{- define "envproxy.injector.fullname" -}}
{{- printf "%s-injector" (include "envproxy.fullname" .) }}
{{- end }}

{{- define "envproxy.agent.fullname" -}}
{{- printf "%s-agent" (include "envproxy.fullname" .) }}
{{- end }}

{{/*
Webhook service name (used by MutatingWebhookConfiguration).
*/}}
{{- define "envproxy.injector.serviceName" -}}
{{- include "envproxy.injector.fullname" . }}
{{- end }}

{{/*
TLS secret name.
*/}}
{{- define "envproxy.injector.tlsSecretName" -}}
{{- if .Values.tls.secretName }}
{{- .Values.tls.secretName }}
{{- else }}
{{- printf "%s-tls" (include "envproxy.injector.fullname" .) }}
{{- end }}
{{- end }}
