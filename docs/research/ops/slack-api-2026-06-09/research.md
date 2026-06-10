# Slack Platform API research for FORTUNA ops alerting — 2026-06-09

Scope: facts needed to build a Rust trading-ops bot that (a) posts channel-routed alerts
to 5 private channels, (b) posts interactive approve/reject buttons and receives the
button callbacks, (c) accepts an authenticated "kill" slash command from Slack, while
(d) guaranteeing a compromised Slack token can never un-halt the system.

All claims below were verified against official Slack docs on **2026-06-09**. Slack's
developer docs migrated from `api.slack.com` to `docs.slack.dev` on 2025-08-28;
`api.slack.com` URLs now 302-redirect to `docs.slack.dev`.

## Sources

All retrieved 2026-06-09 (via redirect from the corresponding `api.slack.com` URL where noted):

| URL | What it covered |
|---|---|
| https://docs.slack.dev/reference/methods/chat.postMessage | postMessage contract (redirect from api.slack.com/methods/chat.postMessage) |
| https://docs.slack.dev/apis/web-api/rate-limits | Tier system, special posting limit, 429/Retry-After, 30k events/hr |
| https://docs.slack.dev/changelog/2025/05/29/rate-limit-changes-for-non-marketplace-apps | 2025 rate-limit change FAQ |
| https://docs.slack.dev/changelog/2025/06/03/rate-limits-clarity/ | Internal-app exemption clarification |
| https://docs.slack.dev/apis/events-api/using-socket-mode | Socket Mode lifecycle, envelopes, acks (redirect from api.slack.com/apis/socket-mode) |
| https://docs.slack.dev/reference/methods/apps.connections.open | WSS URL minting |
| https://docs.slack.dev/interactivity/handling-user-interaction | 3-second ack, response_url semantics |
| https://docs.slack.dev/reference/interaction-payloads/block_actions-payload | Button-click payload shape |
| https://docs.slack.dev/reference/block-kit/blocks (+ section-block, actions-block, block-elements/button-element) | Block/element limits |
| https://docs.slack.dev/interactivity/implementing-slash-commands | Slash command contract |
| https://docs.slack.dev/app-manifests + https://docs.slack.dev/reference/app-manifest | Manifest creation flow and schema |
| https://docs.slack.dev/authentication/tokens | xoxb / xoxp / xapp / config token types |
| https://docs.slack.dev/authentication/verifying-requests-from-slack | Signing secret (HTTP path only) |
| https://docs.slack.dev/apis/web-api/ | Request format, `ok`/`error` envelope |
| https://docs.slack.dev/apis/events-api/ | HTTP retry schedule, failure cutoffs |
| https://docs.slack.dev/quickstart | App creation + token generation flow |
| https://docs.slack.dev/changelog (+ /changelog/future via search) | 2025–2026 deprecations |
| https://docs.slack.dev/reference/scopes/chat.write | chat:write and siblings |

## Sending

**Endpoint:** `POST https://slack.com/api/chat.postMessage`. Accepts
`application/json` (set `Content-type: application/json; charset=utf-8`) or
`application/x-www-form-urlencoded`. Token goes in the `Authorization: Bearer <token>`
header (bot `xoxb-` or user `xoxp-`); when posting JSON the token must NOT be in the
body or query string.

**Scope:** `chat:write` (bot scope; "Send messages as your Slack app"). Siblings:
`chat:write.public` (post to public channels without joining — does NOT apply to private
channels) and `chat:write.customize` (override username/avatar).

**Channel argument:** "An encoded ID or channel name that represents a channel, private
group, or IM channel." Use channel IDs (`C…`) in config — names are mutable and
ID-resolution is where bugs live. **For our 5 private channels the bot must be a member
(invite it with `/invite @bot`)**; posting without membership/visibility returns
`channel_not_found` (the API deliberately does not distinguish "exists but you can't see
it"). `not_in_channel` is returned when the channel is visible but the bot isn't in it.

**Message body:** `text` (fallback/notification text) plus optional `blocks` array.
`mrkdwn` defaults to true. Useful extras: `thread_ts` (reply in thread),
`reply_broadcast`, `unfurl_links` / `unfurl_media`.

**Size limits:** keep `text` ≤ 4,000 chars (docs recommendation); Slack truncates
messages > 40,000 chars. Max 50 blocks per message. Max 100 attachments (legacy).

**Response:** `{"ok": true, "channel": "C123ABC456", "ts": "1503435956.000247",
"message": {…}}` — persist `(channel, ts)`; it is the message's identity for later
`chat.update` / `response_url` correlation.

### Worked curl example (alert with approve/reject)

```bash
curl -sS https://slack.com/api/chat.postMessage \
  -H "Authorization: Bearer ${SLACK_BOT_TOKEN}" \
  -H "Content-type: application/json; charset=utf-8" \
  -d '{
    "channel": "C0123456789",
    "text": "Drawdown gate tripped on alpha-1 (-4.2%). Approval required.",
    "blocks": [
      {
        "type": "section",
        "text": {
          "type": "mrkdwn",
          "text": ":rotating_light: *Drawdown gate tripped* — `alpha-1` at -4.2%.\nApprove flatten-all?"
        }
      },
      {
        "type": "actions",
        "block_id": "halt_approval_01JXAMPLEULID",
        "elements": [
          {
            "type": "button",
            "style": "primary",
            "action_id": "approve_flatten",
            "text": { "type": "plain_text", "text": "Approve" },
            "value": "halt:01JXAMPLEULID:approve"
          },
          {
            "type": "button",
            "style": "danger",
            "action_id": "reject_flatten",
            "text": { "type": "plain_text", "text": "Reject" },
            "value": "halt:01JXAMPLEULID:reject"
          }
        ]
      }
    ]
  }'
# => {"ok":true,"channel":"C0123456789","ts":"1717933200.000100","message":{...}}
```

## Block Kit minimal interactive message

Exact minimal JSON for a text section + two buttons (this is the `blocks` value):

```json
[
  {
    "type": "section",
    "text": { "type": "mrkdwn", "text": "*Approve flatten-all for `alpha-1`?*" }
  },
  {
    "type": "actions",
    "block_id": "halt_approval_01JXAMPLEULID",
    "elements": [
      {
        "type": "button",
        "text": { "type": "plain_text", "text": "Approve" },
        "style": "primary",
        "action_id": "approve_flatten",
        "value": "halt:01JXAMPLEULID:approve"
      },
      {
        "type": "button",
        "text": { "type": "plain_text", "text": "Reject" },
        "style": "danger",
        "action_id": "reject_flatten",
        "value": "halt:01JXAMPLEULID:reject"
      }
    ]
  }
]
```

Verified limits (from the block/element reference pages):

| Field | Limit |
|---|---|
| Blocks per message | 50 (100 in modals/Home tabs) |
| `section.text` | 1–3,000 chars (`mrkdwn` or `plain_text`) |
| `section.fields` | max 10 items, 2,000 chars each |
| `actions.elements` | max 25 elements per actions block |
| `block_id` | max 255 chars; should be unique per message AND changed when updating a message |
| button `text` | `plain_text` only, max 75 chars |
| button `action_id` | max 255 chars, unique within the containing block |
| button `value` | max 2,000 chars |
| button `style` | omit (default), `"primary"` (green), `"danger"` (red) |
| button `confirm` | optional confirmation-dialog object (extra click-through — consider for kill/approve) |

Buttons may live in `actions` blocks or as a `section` `accessory`. 2026 added new
block types (data table 2026-05-20; alert/card/carousel 2026-04-16) — additive,
nothing we rely on changed.

## Socket Mode

**Status 2026-06-09: fully supported and the documented option for exactly our case** —
"use the Events API and interactive features — *without* exposing a public HTTP Request
URL", for developers "working behind a corporate firewall, or who have other security
concerns." Restriction: "Apps using Socket Mode are *not* currently allowed in the
public Slack Marketplace" — irrelevant (we are internal-only) and actually aligned with
our distribution posture.

**Setup:** toggle Socket Mode on in app settings (or `settings.socket_mode_enabled: true`
in the manifest). Generate an **app-level token** (`xapp-…`) under Basic Information →
App-Level Tokens with the **`connections:write`** scope.

**Connection lifecycle:**
1. `POST https://slack.com/api/apps.connections.open` with
   `Authorization: Bearer xapp-…` (token MUST be in the header, not a POST param).
   Tier 3 (50+/min). Response: `{"ok": true, "url": "wss://wss-….slack.com/link/?ticket=…&app_id=…"}`.
2. Connect a WebSocket to that URL; Slack sends a `hello` message with connection
   metadata including `approximate_connection_time`.
3. Up to **10 simultaneous WebSocket connections** per app (events are distributed
   across them — run ≥2 for seamless refresh handover).
4. Slack periodically refreshes connections: expect `disconnect` messages
   (`refresh_requested`), with a warning roughly 10 seconds before the disconnect.
   Reconnect by calling `apps.connections.open` again. Append `&debug_reconnects=true`
   to the WSS URL in dev to shrink connection lifetime to 360 s and exercise the
   reconnect path.

**Envelopes:** every delivery arrives as
`{ "envelope_id": "…", "type": "events_api" | "interactive" | "slash_commands",
"payload": {…}, "accepts_response_payload": bool }`. The `payload` is the same JSON you
would have received at an HTTP Request URL (e.g., a `block_actions` payload for
`type: "interactive"`).

**Ack contract:** send `{"envelope_id": "<id>"}` back over the socket for *each*
envelope — "Your app still needs to acknowledge receiving each event so that Slack knows
whether to retry." When `accepts_response_payload` is true you may ack with
`{"envelope_id": "…", "payload": {…}}` (e.g., an immediate slash-command response).
Treat the 3-second interactivity ack deadline as applying to envelope acks too.

**Verification:** "there's no need to verify or validate inbound events, because you're
receiving the events over a pre-authenticated WebSocket." The signing-secret scheme
(`X-Slack-Signature: v0=HMAC-SHA256(signing_secret, "v0:" + timestamp + ":" + body)`,
`X-Slack-Request-Timestamp` within 5 minutes) applies only to the HTTP Request URL path,
which we are not using.

### Button-click payload (envelope `type: "interactive"`, payload `type: "block_actions"`)

```json
{
  "type": "block_actions",
  "team": { "id": "T9TK3CUKW", "domain": "example" },
  "user": { "id": "UA8RXUSPL", "username": "jtorrance", "team_id": "T9TK3CUKW" },
  "api_app_id": "AABA1ABCD",
  "token": "9s8d9as89d8as9d8as989",
  "container": { "type": "message", "message_ts": "1548261231.000200" },
  "trigger_id": "12321423423.333649436676.d8c1bb837935619ccad0f624c448ffb3",
  "channel": { "id": "CBR2V3XEX", "name": "review-updates" },
  "message": { "bot_id": "BAH5CA16Z", "type": "message", "ts": "1548261231.000200" },
  "response_url": "https://hooks.slack.com/actions/AABA1ABCD/1232321423432/D09sSasdasdAS9091209",
  "actions": [
    {
      "action_id": "approve_flatten",
      "block_id": "halt_approval_01JXAMPLEULID",
      "type": "button",
      "value": "halt:01JXAMPLEULID:approve",
      "action_ts": "1548426417.840180"
    }
  ]
}
```

Authorization-relevant fields: `user.id`, `team.id`, `channel.id`, plus your own
`action_id`/`value`. The top-level `token` is the *deprecated legacy verification
token* — do not use it for auth. `response_url` is NOT deprecated: it accepts up to
**5 POSTs within 30 minutes**, supports `{"replace_original": "true", …}` to rewrite the
original message (e.g., swap buttons for "Approved by @x at T") and
`{"delete_original": "true"}`; default `response_type` is `ephemeral`, use
`"in_channel"` for visible responses. After 30 minutes, use `chat.update` with the
stored `(channel, ts)` instead.

## Slash command for the kill switch

- Defined in app config or manifest (`features.slash_commands`). With Socket Mode on,
  "your app settings doesn't require or even allow you to enter a Request URL" — the
  command arrives as an envelope with `type: "slash_commands"`.
- Invocation payload fields: `command`, `text` (everything after the command — parse for
  e.g. `/fortuna-kill <reason>`), `user_id`, `user_name`, `channel_id`, `team_id`,
  `api_app_id`, `trigger_id`, `response_url`.
- Ack within **3 seconds** or the user sees `operation_timeout`. Over Socket Mode, ack
  the envelope (optionally with a response payload). Default response visibility is
  `ephemeral`; `"response_type": "in_channel"` makes it public.
- **Slack provides no per-user restriction on who may invoke a slash command** — any
  member of the workspace can run it, and command `text` is untrusted input. The handler
  MUST allow-list `user_id` (and ideally `channel_id` and `team_id`) before acting, and
  reply ephemeral "not authorized" otherwise. This satisfies the spec's
  "authenticated and allow-listed" requirement; the allow-list lives in FORTUNA config,
  not Slack.
- Spec-security mapping: the Slack handler should expose **halt-only** semantics. Do not
  implement any re-arm/un-halt verb over Slack at all; then a compromised `xoxb-` token
  (which can only call Web API methods) or `xapp-` token (which can only open sockets
  and receive/ack envelopes) has no code path to un-halt (I2/I4). Note one subtlety: a
  stolen `xapp-` token lets an attacker open competing socket connections and *receive*
  envelopes (DoS/snoop on approvals), since Slack distributes events across the ≤10
  connections. Alert on unexpected connection counts; treat Slack approval buttons as
  advisory inputs that still pass FORTUNA's own gates.
- A message shortcut (envelope payload `type: "message_actions"` / `shortcut`) is an
  alternative trigger surface, but a slash command is the simpler, documented fit.

## Rate limits (current numbers, 2026-06-09)

Standard Web API tiers, applied **per method, per workspace, per app**, per-minute
windows:

| Tier | Limit |
|---|---|
| Tier 1 | 1+ / min |
| Tier 2 | 20+ / min |
| Tier 3 | 50+ / min |
| Tier 4 | 100+ / min |
| Special | method-specific (chat.postMessage) |

**What applies to us:**
- `chat.postMessage` is **Special tier: 1 message/second/channel** ("short bursts >1
  allowed", excessive bursts may fail delivery), plus an app-wide workspace ceiling of
  "several hundred messages per minute" (deliberately unquantified in docs). With 5
  channels, a queue-per-channel pacing at ≤1 msg/s/channel is the correct design.
- `apps.connections.open`: Tier 3 — irrelevant at our reconnect frequency.
- Inbound event deliveries: max **30,000 events/workspace/app/60 min**; beyond that
  Slack sends `app_rate_limited` — far above an alerting bot's volume.
- Exceeding a Web API limit returns **HTTP 429 with `Retry-After: <seconds>`**.

**2025 rate-limit changes — confirmed NOT applicable to us:** effective 2025-05-29,
`conversations.history` and `conversations.replies` dropped to **1 req/min, max 15
objects** for newly created or newly installed **commercially distributed apps not
approved for the Slack Marketplace**. The 2025-06-03 clarification states: "Any internal
customer-built apps will maintain their existing rate limits and will not be subject to
the new posted limits" (internal apps keep 50+/min, up to 1,000 objects). A
single-workspace internal app is in the exempt category, and we don't call those two
methods anyway. **No 2025/2026 change to `chat.postMessage` limits.**

## App setup (single-workspace internal app)

1. Go to `api.slack.com/apps` → **Create New App** → **From a manifest** → pick the
   workspace → paste manifest (JSON or YAML) → Create. Manifest is version-controllable;
   App Manifest APIs (`apps.manifest.*`, using *configuration tokens*) exist for
   automation.
2. Basic Information → **App-Level Tokens** → Generate token with scope
   `connections:write` → store as `SLACK_APP_TOKEN` (`xapp-…`).
3. OAuth & Permissions → **Install to Workspace** → copy **Bot User OAuth Token**
   (`xoxb-…`) → store as `SLACK_BOT_TOKEN`. (Both via env vars only, per repo rules.)
4. `/invite @fortuna-ops` in each of the 5 private alert channels; record their `C…` IDs
   in `config/fortuna.toml`.

**No Marketplace review is required for an internal app** installed to your own
workspace — review applies to Marketplace listing/commercial distribution, and Socket
Mode apps are not Marketplace-eligible anyway. (Workspace admins can restrict app
installs; if so, approval is an in-workspace admin action, not a Slack review.)

Example manifest matching our needs (field names verified against the manifest
reference):

```yaml
display_information:
  name: fortuna-ops
  description: FORTUNA trading-ops alerting (halt-only control surface)
features:
  bot_user:
    display_name: fortuna-ops
    always_online: true
  slash_commands:
    - command: /fortuna-kill
      description: Halt FORTUNA trading (halt-only; re-arm is CLI-only)
      usage_hint: "<reason>"
      should_escape: true
oauth_config:
  scopes:
    bot:
      - chat:write
settings:
  interactivity:
    is_enabled: true
  socket_mode_enabled: true
  org_deploy_enabled: false
  token_rotation_enabled: false
```

(With `socket_mode_enabled: true`, no `request_url`s are required or allowed. Scopes
needed beyond `chat:write`: none for our feature set. Token rotation is optional and
off by default.)

**Token model recap:** `xoxb-` bot token = Web API calls (posting); `xapp-` app-level
token = Socket Mode connections only; signing secret = unused on the Socket Mode path.
Legacy bot/workspace/custom-integration tokens are "no longer supported or recommended."

## Failure semantics

- **Envelope:** every Web API response carries top-level `"ok": true|false`. Failures:
  `{"ok": false, "error": "short_code"}`; partial success may add `"warning"`. Most
  app-level errors arrive with HTTP 200 — **check `ok`, not the status code**.
- **Errors to handle for `chat.postMessage`:** `channel_not_found`, `not_in_channel`
  (bot not invited — surface as ops misconfig, do not retry), `missing_scope`,
  `invalid_auth` / `token_revoked` (auth failure — alert out-of-band), `invalid_blocks`,
  `msg_too_long`, `no_text`, `rate_limited`.
- **Rate limiting:** HTTP 429 + `Retry-After` header (integer seconds). Honor it
  exactly; the limit is scoped to that method+workspace+app, so other methods keep
  working. For an alerting bot, queue and drain rather than drop.
- **Inbound retry / idempotency:** Slack retries undelivered events — over HTTP the
  schedule is immediate, 1 min, 5 min (max 3 retries, `x-slack-retry-num` /
  `x-slack-retry-reason` headers); over Socket Mode, unacked envelopes are retried
  (schedule not published). Therefore **button/command handling must be idempotent**:
  dedupe on `envelope_id` and on your own `value` ULID + `action_ts`; an
  approve/reject for an already-decided halt-approval must be a no-op with an
  explanatory ephemeral reply. HTTP-path apps failing >95% of deliveries in 60 min get
  event subscriptions disabled — keep acks unconditional and fast (ack first, process
  after).
- **Outbound idempotency: there is NO idempotency key on `chat.postMessage`.** A timeout
  after Slack accepted the call can double-post. Mitigate: short client timeout +
  at-most-once send with local ledger of `(alert_id → channel, ts)`; or tolerate rare
  duplicate alerts (preferable to dropping one). Never blind-retry on ambiguous network
  failure for approve/reject prompts — re-check whether the message landed (the stored
  `ts` is the dedupe handle).

## Deprecations relevant in 2026

- **Docs host migration** (2025-08-28): `api.slack.com/docs` → `docs.slack.dev`. Update
  any doc links; API endpoints themselves remain `https://slack.com/api/*`.
- **RTM API:** legacy; not available to modern granular-scope apps at all. Socket Mode is
  its designated replacement. Don't touch any Rust crate built on RTM.
- **Legacy custom bots:** stopped working 2025-03-31. **Classic apps:** retirement was
  scheduled for 2026-11-16, but a 2025-12-08 changelog entry says classic apps "will
  continue to work for the foreseeable future" — moot for us; we are creating a modern
  granular-scope app.
- **`files.upload`:** retired (final extension to 2025-11-12); successor is
  `files.getUploadURLExternal` + `files.completeUploadExternal`. We don't upload files.
- **No scheduled 2026 changes found** affecting `chat.postMessage`, Block Kit buttons,
  `block_actions`, Socket Mode, app-level tokens, or slash commands in the changelog
  through 2026-06-09. 2026 Block Kit changes are additive new block types (alert/card/
  carousel 2026-04-16, data table 2026-05-20); PKCE GA 2026-03-30; optional scopes
  2026-03-16.
- **App-based incoming webhooks** remain a supported feature (we don't need them —
  `chat.postMessage` is strictly more capable and uses the same `1/s/channel` limit).

## Uncertainties

- **Socket Mode retry schedule:** docs confirm unacked envelopes are retried but publish
  no timing, retry count, or dedupe fields for the Socket path (the `retry_attempt` /
  `retry_reason` envelope fields seen in SDKs are not in the page fetched). Design for
  at-least-once delivery regardless.
- **WSS URL lifetime:** `apps.connections.open` docs don't state a TTL for the ticket
  URL. Treat it as single-use and mint a fresh URL on every (re)connect.
- **`connections:write` wording conflict:** the `apps.connections.open` reference says
  "No scopes required" while the Socket Mode guide and quickstart instruct generating the
  app-level token *with* `connections:write`. Practical rule: generate the `xapp-` token
  with `connections:write` (the reference page's note appears to mean no *bot/user*
  scopes).
- **Workspace-wide posting ceiling** is documented only as "several hundred messages per
  minute" — no exact number published. Irrelevant at our volumes if per-channel pacing
  is in place.
- **Interactivity ack over Socket Mode:** the 3-second deadline is documented for the
  HTTP path; the Socket Mode page demands per-envelope acks but doesn't restate the
  window. Assume 3 s.
- **Classic-app retirement date** is ambiguous (2026-11-16 scheduled vs 2025-12-08
  "foreseeable future" pause). Does not affect a newly created granular app.
- **Admin-approval flow** for installing apps depends on the workspace's admin settings;
  not determinable from API docs.
- One fetch summarizer described `block_actions.response_url` as "deprecated" — this was
  cross-checked against the reference page and search results and is **wrong**; only the
  legacy `token` verification field is deprecated. Recorded here so the error isn't
  re-inherited.
