> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Onboarding

> Get started with the Polymarket Exchange API

<Note>**Individual traders:** You do not need to complete this onboarding process. Head to the [Retail Trading](/retail-api/overview) tab to get started.</Note>

## Step 1: Generate Your Key Pairs

Generate an RSA key pair for each environment you need access to. You will share only the **public keys** with Polymarket.

```bash theme={null}
# Replace 'acme' with your company name

# Development
openssl genrsa -out acme_dev_private_key.pem 2048
openssl rsa -in acme_dev_private_key.pem -pubout -out acme_dev_public_key.pem

# Pre-production
openssl genrsa -out acme_preprod_private_key.pem 2048
openssl rsa -in acme_preprod_private_key.pem -pubout -out acme_preprod_public_key.pem

# Production
openssl genrsa -out acme_prod_private_key.pem 2048
openssl rsa -in acme_prod_private_key.pem -pubout -out acme_prod_public_key.pem
```

Keep your private keys secure. Never share them with anyone.

### API-Specific Requirements

<AccordionGroup>
  <Accordion title="REST / gRPC">
    No additional setup is required beyond the RSA key pairs generated above.
  </Accordion>

  <Accordion title="FIX">
    If you intend to use the FIX API, please also include your **AWS Account ID** in your submission so that a VPC PrivateLink connection can be established.

    You should still generate and submit RSA key pairs even if FIX is your primary connectivity method, as they are required for full platform functionality.
  </Accordion>
</AccordionGroup>

## Step 2: Submit Your Onboarding Request

Download and complete the required onboarding documents:

[Entity Participant Agreement](https://www.polymarketexchange.com/files/legal/Polymarket%20-%20Entity%20Participant%20and%20Clearing%20Member%20Agreement%20\(2026.05.20\).pdf)

Create a Google Drive folder containing your public key file(s) and completed document, then email **[onboarding@polymarket.us](mailto:onboarding@polymarket.us)** with your name or the name of your firm and a Google Drive link to your folder (grant Editor access to `onboarding@polymarket.us`).

If you are requesting FIX connectivity, also email **[fix@polymarket.us](mailto:fix@polymarket.us)** with your firm name, AWS Account ID, and a link to the same folder.

## Step 3: Receive Your Credentials

The Polymarket team will review your submission and provide your Client ID credentials via email for both pre-production and production environments.

If you requested FIX connectivity, you will also receive your FIX connection details. See [FIX Connection Setup](/institutional/fix-api/fix-connection-setup) for VPC endpoint configuration and session setup.

## Step 4: Fund Your Account

Your pre-production account will be funded with dummy funds for testing purposes. To begin trading on the production environment, fund your account via wire transfer:

* [Inbound Wire Form](https://drive.google.com/uc?export=download\&id=1Y8mPOuYawVen42eJ61NUrY6ERlfMPrS6) - Use this form to wire funds into your Polymarket account
* [Outbound Wire Form](https://drive.google.com/uc?export=download\&id=1TgiahAwVHd59VU5t5XP4gyd6hFYuhmSz) - Use this form to withdraw funds from your Polymarket account

Complete the appropriate form and follow the wire instructions provided. Funds are typically available for trading within 1-2 business days of receipt.
