# envproxy-injector: mutating admission webhook for Kubernetes
# Supports multi-arch builds (amd64, arm64) via buildx TARGETARCH.
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
FROM alpine:3.21

RUN apk add --no-cache ca-certificates

COPY --from=builder /injector /injector

USER nobody:nobody
ENTRYPOINT ["/injector"]
