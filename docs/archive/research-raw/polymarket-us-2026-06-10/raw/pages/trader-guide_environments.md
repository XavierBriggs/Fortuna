> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Environments

> API endpoints for dev, preprod, and production environments

The Polymarket Exchange API is available in three environments. Use the appropriate endpoints based on your integration stage.

## Endpoints

| Environment | Purpose                   | REST API                                     | gRPC                                      | Auth Domain                |
| ----------- | ------------------------- | -------------------------------------------- | ----------------------------------------- | -------------------------- |
| **Dev**     | Development and testing   | `https://api.dev01.polymarketexchange.com`   | `grpc-dev01.polymarketexchange.com:443`   | `pmx-dev01.us.auth0.com`   |
| **Preprod** | Pre-production validation | `https://api.preprod.polymarketexchange.com` | `grpc-preprod.polymarketexchange.com:443` | `pmx-preprod.us.auth0.com` |
| **Prod**    | Production trading        | `https://api.prod.polymarketexchange.com`    | `grpc-prod.polymarketexchange.com:443`    | `pmx-prod.us.auth0.com`    |

The Audience value for each environment matches its REST API base URL.

### API Path Patterns

| API Type       | Path Pattern                | Example                                             |
| -------------- | --------------------------- | --------------------------------------------------- |
| REST endpoints | `/v1/{service}/{operation}` | `/v1/trading/orders`                                |
| Health check   | `/v1/health`                | `GET /v1/health`                                    |
| Auth token     | `/oauth/token`              | `POST https://pmx-preprod.us.auth0.com/oauth/token` |

### Authentication

| Environment | Token URL                                      |
| ----------- | ---------------------------------------------- |
| Dev         | `https://pmx-dev01.us.auth0.com/oauth/token`   |
| Preprod     | `https://pmx-preprod.us.auth0.com/oauth/token` |
| Prod        | `https://pmx-prod.us.auth0.com/oauth/token`    |

<Warning>
  **Tokens must be refreshed every 3 minutes** across all environments.
</Warning>

## Health Check

All environments expose the same health check endpoint:

```bash theme={null}
curl https://api.dev01.polymarketexchange.com/v1/health
```

```json theme={null}
{
  "status": "SERVING",
  "version": "1.0.0"
}
```

## AWS PrivateLink Connection

<Warning>
  **VPC connections are required only for FIX API access.** REST and gRPC APIs use the public endpoints listed above and do not require VPC setup.
</Warning>

For secure, private connectivity from your AWS VPC for the FIX API, use AWS PrivateLink. This routes traffic over AWS's private network instead of the public internet.

VPC Service Names and PrivateLink endpoints are provisioned per-firm during FIX onboarding. Contact [onboarding@polymarket.us](mailto:onboarding@polymarket.us) with your AWS Account ID to get started.

## Environment Progression

We recommend the following integration progression:

1. **Dev** - Initial development and unit testing
2. **Preprod** - Integration testing and pre-launch validation
3. **Prod** - Live trading

<Info>
  Contact [onboarding@polymarket.us](mailto:onboarding@polymarket.us) to request access credentials for each environment.
</Info>
