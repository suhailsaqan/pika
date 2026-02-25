---
summary: Practical guide for bot developers sending hypernotes (MDX + interactive UI) via the pikachat daemon
read_when:
  - building a bot that sends rich UI via pikachat
  - integrating hypernotes into an OpenClaw extension
  - writing an LLM system prompt for hypernote generation
---

# Hypernote Bot Guide

How to send rich, interactive UI from a bot over the pikachat daemon.

## Quick start

Send a hypernote via the daemon's JSONL stdin:

```json
{"cmd": "send_hypernote", "nostr_group_id": "<hex>", "content": "# Hello\n\nThis is **bold** and *italic*."}
```

The client renders it as native SwiftUI — not a web view, not a markdown blob.

## The `send_hypernote` command

```json
{
  "cmd": "send_hypernote",
  "request_id": "optional-correlation-id",
  "nostr_group_id": "<hex group id>",
  "content": "<MDX source>",
  "actions": "<optional JSON string>",
  "title": "<optional display title>",
  "state": "<optional JSON string>"
}
```

| Field | Required | Description |
|-------|----------|-------------|
| `nostr_group_id` | yes | Hex group ID (from `init_group` or `group_joined` event) |
| `content` | yes | MDX source — markdown with optional JSX components |
| `actions` | no | JSON string mapping action names to signed-action definitions |
| `title` | no | Display title (stored as a tag, not part of rendered content) |
| `state` | no | JSON string of default state values for interactive components |

Response: `{"type": "ok", "request_id": "...", "result": {"event_id": "..."}}`

## MDX syntax

Hypernote content is standard markdown extended with JSX components. The parser (`hypernote-mdx`) handles the full CommonMark subset plus JSX elements.

### Markdown (all standard features)

```markdown
# Heading 1
## Heading 2
### Heading 3

Regular paragraph with **bold**, *italic*, and `inline code`.

- Unordered list item
- Another item

1. Ordered list
2. Second item

> Blockquote

[Link text](https://example.com)

![Alt text](https://example.com/image.png)

---

\`\`\`rust
fn main() { println!("code block"); }
\`\`\`
```

### JSX components

Components are mixed inline with markdown. Self-closing (`<Component />`) and container (`<Component>...</Component>`) forms both work.

```jsx
# Order Summary

<Card>
  <Caption>Total</Caption>
  <Heading>50,000 sats</Heading>
</Card>

<SubmitButton action="confirm">Pay Now</SubmitButton>
```

## Component catalog

### Layout

| Component | Props | Description |
|-----------|-------|-------------|
| `Card` | — | Rounded container with subtle background |
| `VStack` | `gap` (number, default 8) | Vertical stack |
| `HStack` | `gap` or `spacing` (number, default 8) | Horizontal stack |

### Typography

| Component | Props | Description |
|-----------|-------|-------------|
| `Heading` | — | Bold headline text |
| `Body` | — | Standard body text |
| `Caption` | — | Muted secondary text |

### Interactive

| Component | Props | Description |
|-----------|-------|-------------|
| `ChecklistItem` | `name` (required), `checked` (optional flag) | Interactive checkbox bound to form state |
| `TextInput` | `name` (required), `placeholder` (optional) | Text field bound to form state |
| `SubmitButton` | `action` (required), `variant` (optional) | Button that triggers an action |

**SubmitButton variants:**

| Variant | Default style | After submit (selected) | After submit (unselected) |
|---------|--------------|------------------------|--------------------------|
| `primary` (default) | Prominent blue | Prominent + checkmark | Bordered, 50% opacity |
| `secondary` | Bordered | Prominent + checkmark | Bordered, 50% opacity |
| `danger` | Prominent red | Prominent red + checkmark | Bordered, 50% opacity |

**Form disable behavior:** After any SubmitButton is tapped, all interactive elements in the hypernote are disabled. The selected button shows a checkmark icon; unselected buttons fade to 50% opacity. A "Response sent" confirmation appears at the bottom. This prevents double-submission and gives clear visual feedback.

**One form per hypernote.** Each hypernote should have a single clear purpose — don't mix unrelated inputs. The bot only sees form data when the user taps a SubmitButton, so all interactive elements in a hypernote should relate to the same action. For multi-step flows, send separate hypernotes for each step.

### Unknown components

Any unrecognized component name renders its children with a dashed border. Content is never lost — you can use custom component names as semantic markers and they'll degrade gracefully.

## Actions

When a user taps a `SubmitButton`, the action system has two tiers:

### Chat actions (default)

If the `action` name is **not** defined in the `actions` tag, the client sends a hypernote action response (inner kind `9468`) back to the group:

```json
{"action": "confirm", "form": {"name": "Alice", "amount": "1000"}}
```

Your bot receives this as a `message_received` event. The response uses kind 9468 (not a regular chat message), so it is **not visible in the chat timeline** — the user won't see a raw JSON blob. The bot still receives it normally via the daemon.

**This is what most bots should use.** It's simple, requires no extra setup, and the bot gets structured form data back without cluttering the chat.

### Signed actions (advanced)

If the `action` name **is** defined in the `actions` JSON tag, the client constructs and signs a real Nostr event with the user's key. This is for actions with real-world weight — payments, votes, attestations.

```json
{
  "cmd": "send_hypernote",
  "nostr_group_id": "abc123",
  "content": "# Vote\n\n<SubmitButton action=\"vote_yes\">Yes</SubmitButton>",
  "actions": "{\"vote_yes\":{\"kind\":7,\"content\":\"approved\",\"tags\":[[\"e\",\"<proposal-event-id>\"]]}}"
}
```

Action definition schema:

```json
{
  "kind": 7,
  "content": "template with {{form.fieldName}} and {{user.pubkey}}",
  "tags": [["tag-name", "{{form.value}}"]],
  "confirm": "Optional confirmation prompt shown to user"
}
```

Template variables:
- `{{form.fieldName}}` — value from the form's TextInput with that name
- `{{user.pubkey}}` — the user's hex pubkey
- `{{bot.pubkey}}` — the bot's hex pubkey (from the message sender)

## Examples

### Simple question

```json
{
  "cmd": "send_hypernote",
  "nostr_group_id": "abc123",
  "content": "# What would you like to do?\n\n<HStack gap=\"8\">\n  <SubmitButton action=\"check_balance\">Check Balance</SubmitButton>\n  <SubmitButton action=\"send_payment\">Send Payment</SubmitButton>\n</HStack>"
}
```

### Form with text input

```json
{
  "cmd": "send_hypernote",
  "nostr_group_id": "abc123",
  "content": "# Send Payment\n\n<Card>\n  <TextInput name=\"recipient\" placeholder=\"npub or lightning address\" />\n  <TextInput name=\"amount\" placeholder=\"Amount in sats\" />\n  <TextInput name=\"memo\" placeholder=\"Memo (optional)\" />\n</Card>\n\n<SubmitButton action=\"send\" variant=\"danger\">Send Sats</SubmitButton>"
}
```

The bot receives back:
```json
{"action": "send", "form": {"recipient": "npub1...", "amount": "1000", "memo": "thanks"}}
```

### Checklist with submit

```json
{
  "cmd": "send_hypernote",
  "nostr_group_id": "abc123",
  "content": "# Today's Tasks\n\n<ChecklistItem name=\"groceries\">Buy groceries</ChecklistItem>\n<ChecklistItem name=\"tests\">Write tests</ChecklistItem>\n<ChecklistItem name=\"deploy\" checked>Deploy to staging</ChecklistItem>\n\n<SubmitButton action=\"save\">Save</SubmitButton>"
}
```

The bot receives back:
```json
{"action": "save", "form": {"groceries": "true", "tests": "false", "deploy": "true"}}
```

### Default state

Pre-seed form values using the `state` field:

```json
{
  "cmd": "send_hypernote",
  "nostr_group_id": "abc123",
  "content": "# Settings\n\n<TextInput name=\"threshold\" placeholder=\"Alert threshold\" />\n\n<SubmitButton action=\"save\">Save</SubmitButton>",
  "state": "{\"threshold\": \"100\"}"
}
```

The TextInput will show "100" as its initial value.

### Info card (no interaction)

```json
{
  "cmd": "send_hypernote",
  "nostr_group_id": "abc123",
  "content": "# Weather Report\n\n<Card>\n  <Heading>72\u00b0F / Sunny</Heading>\n  <Body>San Francisco, CA</Body>\n  <Caption>Updated 5 min ago</Caption>\n</Card>\n\nHigh: 75\u00b0F | Low: 58\u00b0F\nWind: 12 mph NW"
}
```

## LLM system prompt snippet

If your bot uses an LLM to generate hypernotes, include this in the system prompt:

```
You can send rich UI to the user using hypernote — markdown with JSX components.

Available components:
- <Card> — rounded container
- <VStack gap="N"> / <HStack gap="N"> — layout (default gap: 8)
- <Heading>, <Body>, <Caption> — typography
- <ChecklistItem name="key" checked>Label</ChecklistItem> — interactive checkbox
- <TextInput name="fieldName" placeholder="hint" /> — text input
- <SubmitButton action="actionName" variant="primary|secondary|danger">Label</SubmitButton>

Rules:
- Use standard markdown for text (headings, bold, italic, lists, links, code blocks)
- Use <ChecklistItem> for interactive checklists — each needs a `name` prop, add `checked` for pre-checked
- Use components for interactive elements and structured layouts
- Every SubmitButton needs an `action` prop — this is the name your bot receives back
- Every TextInput needs a `name` prop — this becomes a key in the form data
- When the user taps a SubmitButton, you receive: {"action": "actionName", "form": {"fieldName": "value", "checkName": "true", ...}}
- After submit, all inputs, checkboxes, and buttons are disabled (no double-submit)
- The user's response is NOT visible in chat — only your bot receives it
- ONE form per hypernote. Don't mix unrelated inputs. For multi-step flows, send a new hypernote for each step.
- Keep it simple. A heading + a few buttons is usually enough.
- Don't nest components deeper than 3 levels.
```

## Inner event format (reference)

Hypernotes use inner kind `9467` inside MLS-encrypted messages (kind 443 wrapper). The inner rumor structure:

```json
{
  "kind": 9467,
  "content": "# MDX source here\n\n<Card>...</Card>",
  "tags": [
    ["actions", "{\"action_name\": {\"kind\": 7, \"content\": \"...\", \"tags\": [...]}}"],
    ["title", "Optional title"],
    ["state", "{\"key\": \"value\"}"]
  ]
}
```

The client parses `content` through `hypernote-mdx` into a JSON AST, then renders it natively in SwiftUI. The `actions`, `title`, and `state` tags are optional metadata.

### Action response (kind 9468)

When a user submits a chat action (taps a SubmitButton with no signed-action definition), the client sends an inner kind `9468` event:

```json
{
  "kind": 9468,
  "content": "{\"action\": \"confirm\", \"form\": {\"name\": \"Alice\"}}",
  "tags": []
}
```

Kind 9468 is stored by MDK and forwarded to the daemon as `message_received`, but it is **excluded from the chat timeline** — the user never sees the raw JSON. This keeps the conversation clean while still delivering structured data to the bot.

## Daemon protocol context

See `docs/fold-marmotd-into-pika-cli.md` for the full daemon JSONL protocol reference, including all commands, events, and lifecycle management.
