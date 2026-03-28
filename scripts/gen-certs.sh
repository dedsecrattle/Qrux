#!/usr/bin/env bash
# Generate self-signed certificates for local development

set -e

CERT_DIR="${1:-./certs}"
DOMAIN="${2:-localhost}"

mkdir -p "$CERT_DIR"

echo "Generating self-signed certificate for $DOMAIN..."

openssl req -x509 \
    -newkey rsa:4096 \
    -sha256 \
    -days 365 \
    -nodes \
    -keyout "$CERT_DIR/key.pem" \
    -out "$CERT_DIR/cert.pem" \
    -subj "/CN=$DOMAIN" \
    -addext "subjectAltName=DNS:$DOMAIN,DNS:*.example.com,DNS:api.example.com,DNS:ws.example.com,IP:127.0.0.1"

echo "Certificates generated in $CERT_DIR/"
echo "  - cert.pem: Certificate"
echo "  - key.pem:  Private key"
