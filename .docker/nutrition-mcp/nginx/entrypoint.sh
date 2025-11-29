#!/bin/sh
set -e

# Always remove any existing HTTPS config first to ensure clean state
rm -f /etc/nginx/conf.d/https.conf

# Check if SSL certificates exist
if [ -f /etc/nginx/ssl/cert.pem ] && [ -f /etc/nginx/ssl/key.pem ]; then
    echo "SSL certificates found - enabling HTTPS"
    # Copy HTTPS configuration to conf.d
    cp /etc/nginx/nginx-https.conf.inactive /etc/nginx/conf.d/https.conf
else
    echo "No SSL certificates found - HTTP only mode"
    # Ensure HTTPS config is removed (already done above, but be explicit)
    rm -f /etc/nginx/conf.d/https.conf
fi

# Test nginx configuration before starting
nginx -t || {
    echo "ERROR: nginx configuration test failed"
    exit 1
}

# Execute the default nginx entrypoint
exec /docker-entrypoint.sh "$@"

