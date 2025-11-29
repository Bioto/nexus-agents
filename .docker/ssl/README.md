# SSL Certificate Setup for Cloudflare Origin Certificate

This directory should contain your Cloudflare Origin Certificate and private key.

## Required Files

1. **cert.pem** - The Cloudflare Origin Certificate (you have this)
2. **key.pem** - The private key that was generated when you created the certificate

## Setup Instructions

1. Create the certificate file:
   ```bash
   # Save the certificate you received from Cloudflare
   cat > .docker/ssl/cert.pem << 'EOF'
   -----BEGIN CERTIFICATE-----
   [paste your certificate here]
   -----END CERTIFICATE-----
   EOF
   ```

2. Add the private key:
   ```bash
   # Save the private key (you should have received this when creating the certificate)
   cat > .docker/ssl/key.pem << 'EOF'
   -----BEGIN PRIVATE KEY-----
   [paste your private key here]
   -----END PRIVATE KEY-----
   EOF
   ```

   **OR** if your private key is in RSA format:
   ```bash
   cat > .docker/ssl/key.pem << 'EOF'
   -----BEGIN RSA PRIVATE KEY-----
   [paste your private key here]
   -----END RSA PRIVATE KEY-----
   EOF
   ```

3. Set proper permissions:
   ```bash
   chmod 644 .docker/ssl/cert.pem
   chmod 600 .docker/ssl/key.pem
   ```

## Finding Your Private Key

If you created the certificate in the Cloudflare dashboard:
- Go to SSL/TLS → Origin Server
- Find your certificate
- Click "View" or "Download" - you should see both the certificate and private key

If you generated it via API or command line, the private key should have been provided at that time.

## Verification

After adding both files, restart nginx:
```bash
docker compose -f .docker/docker-compose.nutrition-mcp.yaml restart nginx
```

Check the logs to confirm HTTPS is enabled:
```bash
docker compose -f .docker/docker-compose.nutrition-mcp.yaml logs nginx
```

You should see: "SSL certificates found - enabling HTTPS"

## Cloudflare SSL Mode

Make sure your Cloudflare SSL/TLS encryption mode is set to:
- **Full** (recommended) - Cloudflare connects to origin via HTTPS
- **Full (strict)** - Same as Full, but requires valid certificate (works with Origin Certificate)

Do NOT use "Flexible" mode with this setup, as it will cause SSL issues.






