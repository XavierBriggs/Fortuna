> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Authentication

> Resolving authentication and authorization issues

<Info>
  For authentication setup and code examples, see the main [Authentication](/trader-guide/authentication) guide.
</Info>

## Common Authentication Errors

### "401 Unauthorized"

Your JWT token is missing, invalid, or expired.

**Check:**

1. Is the token included in the `Authorization: Bearer <token>` header?
2. Has the token expired? Tokens are typically valid for 180 seconds (3 minutes)
3. Is the token format correct? Should be `Bearer eyJ...`
4. Was the token issued by the correct Auth0 domain for your environment?

**Solution**: Request a new access token from Auth0 and retry your request.

**Decode your token** at jwt.io to verify claims and expiration.

### "403 Forbidden"

Your token is valid but doesn't have the required scope for the endpoint.

**Check:**

1. Decode your token at jwt.io to see which scopes you have
2. Verify the endpoint requires a scope you have (see [Authentication scopes](/trader-guide/authentication#api-scopes))
3. Scopes must be space-separated in the `scope` claim (e.g., `"read:orders write:orders"`)

**Solution**: Contact support to request additional scopes for your Client ID.

### "invalid\_client"

JWT signature verification failed when requesting an access token from Auth0.

**Causes:**

* Private key doesn't match the public key registered with Polymarket
* Wrong private key file being used
* Private key file corrupted or invalid format

**Solution**: Verify your private key matches the public key you submitted during onboarding. If keys are mismatched, you'll need to re-onboard with the correct public key.

### "invalid\_client\_assertion"

The client assertion JWT is malformed or has incorrect claims.

**Common causes:**

* Wrong `aud` claim (must be `https://pmx-{env}.us.auth0.com/oauth/token`, NOT the API URL)
* Expired `exp` claim (expiration in the past)
* Missing required claims (`iss`, `sub`, `aud`, `iat`, `exp`, `jti`)
* Reused `jti` (must be unique for each request)
* Wrong signing algorithm (must be RS256)

**Debug by decoding your client assertion JWT:**

```python theme={null}
import jwt

# Decode without verification to inspect claims
decoded = jwt.decode(your_assertion, options={"verify_signature": False})
print(decoded)
```

**Required claims:**

```json theme={null}
{
  "iss": "YOUR_CLIENT_ID",
  "sub": "YOUR_CLIENT_ID",
  "aud": "https://pmx-preprod.us.auth0.com/oauth/token",
  "iat": 1703270400,
  "exp": 1703270700,
  "jti": "unique-uuid-per-request"
}
```

## Environment-Specific Issues

### Using Wrong Environment Credentials

**Problem**: Using development credentials in production (or vice versa).

**Symptoms:**

* `invalid_client` errors
* `401 Unauthorized` on API calls
* Token works in one environment but not another

**Solution**: Each environment requires separate credentials:

* Separate key pairs (different public/private keys)
* Separate Client IDs
* Separate Auth0 domains
* Separate API audiences

| Environment        | Auth Domain                | API Audience                                 |
| ------------------ | -------------------------- | -------------------------------------------- |
| **Development**    | `pmx-dev01.us.auth0.com`   | `https://api.dev01.polymarketexchange.com`   |
| **Pre-production** | `pmx-preprod.us.auth0.com` | `https://api.preprod.polymarketexchange.com` |
| **Production**     | `pmx-prod.us.auth0.com`    | `https://api.prod.polymarketexchange.com`    |

### Cannot Reuse Keys Across Environments

**Environments are completely isolated.** You cannot:

* Use the same private key in multiple environments
* Use preprod credentials in production
* Transfer Client IDs between environments

Each environment requires a complete separate onboarding.

## Token Expiration Issues

### Token Works Sometimes But Not Others

**Cause**: Token is expiring mid-session.

**Solution**: Implement proactive token refresh:

```python theme={null}
class TokenManager:
    def __init__(self, auth_client):
        self.auth_client = auth_client
        self.token = None
        self.token_expiry = 0

    def get_valid_token(self):
        # Refresh if expired or expiring soon (30 second buffer)
        if time.time() >= self.token_expiry - 30:
            self.token = self.auth_client.get_token()
            self.token_expiry = time.time() + 180  # 3 minutes
        return self.token
```

### Token Expired Immediately After Creation

**Cause**: System clock is incorrect.

**Check:** Is your system time synchronized?

```bash theme={null}
date
# Compare with actual time
```

**Solution**: Synchronize your system clock using NTP.

## JWT Claim Issues

### Wrong Audience Claim

**Common mistake**: Using API URL as the `aud` claim in the **client assertion**.

**Incorrect:**

```json theme={null}
{
  "aud": "https://api.preprod.polymarketexchange.com"  // Wrong!
}
```

**Correct:**

```json theme={null}
{
  "aud": "https://pmx-preprod.us.auth0.com/oauth/token"  // Correct
}
```

The audience for the **client assertion** JWT must be the Auth0 token endpoint, not the API.

The **access token** you receive will have the API URL as its audience.

### Reused JTI

**Problem**: Using the same `jti` (JWT ID) for multiple requests.

**Cause**: `jti` is meant to prevent replay attacks and must be unique per request.

**Solution**: Generate a new UUID for each token request:

```python theme={null}
import uuid
claims["jti"] = str(uuid.uuid4())
```

## Verifying Token Claims

Decode your access token to check what scopes and claims you have:

```python theme={null}
import base64
import json

def decode_token(access_token):
    # Split token and get payload
    payload = access_token.split('.')[1]
    # Add padding if needed
    payload += '=' * (4 - len(payload) % 4)
    # Decode
    claims = json.loads(base64.urlsafe_b64decode(payload))

    print("Token expires:", claims.get("exp"))
    print("Granted scopes:", claims.get("scope", ""))
    print("Audience:", claims.get("aud"))
    print("Issuer:", claims.get("iss"))

    return claims

claims = decode_token(your_access_token)
```

## Authorization vs Authentication

**Authentication** proves who you are (which firm).

**Authorization** determines what you can do (which accounts, which scopes).

**You may be authenticated but not authorized** to:

* Access specific trading accounts
* Call certain endpoints (missing scopes)
* Perform certain actions (insufficient permissions)

If you get 403 errors despite being authenticated, it's an authorization issue, not authentication.

## Missing or Incorrect x-participant-id

### Errors on Trading, Positions, or Report Endpoints

**Problem**: Requests to account-scoped endpoints fail even though your access token is valid.

**Cause**: The `x-participant-id` header is missing or contains an incorrect value. This header is required for all account-scoped endpoints (trading, positions, reports) but is **not** required for market data, order book, or reference data endpoints.

**Solution**: Include the `x-participant-id` header in your requests:

```bash theme={null}
curl -X POST "https://api.preprod.polymarketexchange.com/v1/trading/orders" \
  -H "Authorization: Bearer YOUR_ACCESS_TOKEN" \
  -H "Content-Type: application/json" \
  -H "x-participant-id: firms/YourFirm/users/your-user" \
  -d '{ ... }'
```

### Finding Your Participant ID

**Institutional traders (DMA):** Your participant ID is provided during onboarding. To discover or verify it:

* Call `GET /v1/whoami` to see your firm identity
* Call `GET /v1/users` to list all users and their IDs

**Brokers/Partners:** Participant IDs for your end users are returned upon KYC approval. To list them:

* Call `GET /v1/users` to see all users you can trade on behalf of

See [Accounts & Identity](/trader-guide/accounts-identity) for more details.

### Endpoints That Do NOT Require x-participant-id

These endpoints only require a valid access token with the appropriate scope:

* Market data: `/v1/orderbook/*`, market data streaming
* Reference data: `/v1/refdata/*`
* Trade statistics: `/v1/report/trades/stats`
* Health check: `/v1/health`

## Cryptographic Issues

### Wrong Key Format

**Private keys must be in PEM format:**

```
-----BEGIN RSA PRIVATE KEY-----
MIIEpAIBAAKCAQEA...
...
-----END RSA PRIVATE KEY-----
```

If your key is in a different format (DER, JWK, etc.), convert it to PEM:

```bash theme={null}
# Convert DER to PEM
openssl rsa -inform DER -in key.der -out key.pem
```

### Wrong Algorithm

**You must use RS256** (RSA with SHA-256) to sign your JWT.

**Don't use:**

* HS256 (HMAC - symmetric key)
* Other RSA variants (RS384, RS512, PS256, etc.)

## Testing Authentication

Test your authentication setup:

**1. Verify you can create a client assertion:**

```bash theme={null}
# Decode and inspect your assertion
echo "YOUR_ASSERTION_JWT" | cut -d. -f2 | base64 -d | jq
```

**2. Test token request:**

```bash theme={null}
curl -X POST https://pmx-preprod.us.auth0.com/oauth/token \
  -H "Content-Type: application/json" \
  -d '{
    "client_id": "YOUR_CLIENT_ID",
    "client_assertion_type": "urn:ietf:params:oauth:client-assertion-type:jwt-bearer",
    "client_assertion": "YOUR_SIGNED_JWT",
    "audience": "https://api.preprod.polymarketexchange.com",
    "grant_type": "client_credentials"
  }'
```

**3. Test API call with access token:**

```bash theme={null}
curl -X GET https://api.preprod.polymarketexchange.com/v1/whoami \
  -H "Authorization: Bearer YOUR_ACCESS_TOKEN"
```

## Getting Help

If authentication issues persist:

1. Verify your credentials match your environment
2. Decode and inspect your JWTs (client assertion and access token)
3. Check system clock is synchronized
4. Test with curl commands to isolate client code issues
5. Contact support with:
   * Environment (dev, preprod, prod)
   * Client ID
   * Error messages
   * Decoded JWT claims (never share your private key!)
