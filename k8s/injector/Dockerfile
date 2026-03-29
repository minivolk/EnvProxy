# envproxy-injector: mutating admission webhook for Kubernetes
# Supports multi-arch builds (amd64, arm64) via buildx TARGETARCH.
# Runtime is distroless/static — Go binary is fully static (CGO_ENABLED=0).
FROM golang:1.26-alpine AS builder

ARG TARGETOS=linux
ARG TARGETARCH

WORKDIR /src
COPY go.mod go.sum ./
RUN go mod download

COPY . .
RUN CGO_ENABLED=0 GOOS=${TARGETOS} GOARCH=${TARGETARCH} \
    go build -ldflags="-s -w" -o /injector ./cmd/

# -------------------------------------------------------------------
FROM gcr.io/distroless/static:nonroot

COPY --from=builder /injector /injector

USER nonroot:nonroot
ENTRYPOINT ["/injector"]
