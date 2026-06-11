> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# General FAQs

> Frequently asked questions about trading on Polymarket US

### What are the trading hours?

Polymarket US operates nearly 24/7, with a recurring weekly maintenance window every Thursday from 6am–8am ET. Specific markets may have different trading hours based on the underlying event.

### How do I fund my account?

Deposit via debit card or bank transfer (ACH) through the Polymarket US app. See the app's funding section for deposit limits and processing times.

### How do I get support?

Email [support@polymarket.us](mailto:support@polymarket.us) or use the in-app chat.

### When are maintenance windows?

Every Thursday, 6am–8am ET is the recurring weekly maintenance window, effective April 16, 2026. Previously, the window was every Tuesday, 4am–6am ET.

### What happens to open orders during maintenance?

All open orders are canceled before maintenance begins. Leaving resting orders on the book during maintenance would expose traders to stale fills when the book reopens.

### What happens to connections during maintenance?

All API requests return **503 Service Unavailable** during maintenance. An explicit rejection is preferable to leaving connections open but non-functional.

### When do markets reopen?

Connections are re-enabled first, then markets move from SUSPENDED to OPEN. Order books reopen empty since all orders were cancelled before maintenance. The state change signals that maintenance is complete.
