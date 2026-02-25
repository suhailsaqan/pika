---
summary: Integration plan for hypernote — generative UI for Nostr bots rendered natively in SwiftUI with cryptographic authorization
read_when:
  - working on hypernote integration
  - adding new inner event kinds to MLS messages
  - building bot-generated interactive UI
  - modifying message rendering in SwiftUI
status: implemented
---

# Hypernote in Pika: Integration Plan

Generative UI for Nostr bots, rendered natively in a SwiftUI chat app with cryptographic authorization.

## Design Decision: New Inner Kind (Approach A)

We add hypernote as a **new inner event kind** alongside the existing kinds (`Kind::ChatMessage`, `Kind::Reaction`, `Kind::Custom(20_067)` typing, `Kind::Custom(10)` call signal). The hypernote renderer is a separate code path from the existing MarkdownUI-based renderer. Regular messages continue to render exactly as before.

**Why not parse all messages through hypernote?** The alternative — making every `Kind::ChatMessage` go through `hypernote-mdx` and replacing MarkdownUI — is the "cleanest" end state but has unacceptable merge risk. It requires reimplementing MarkdownUI's formatting quality from scratch, migrating the existing `pika-prompt` (polls) and `pika-html` (interactive HTML via WKWebView) systems, and touching every message rendering callsite in Swift. If the hypernote renderer has a bug, all messages break.

**Path to convergence:** Once the hypernote renderer is battle-tested on bot messages, we can later start parsing `Kind::ChatMessage` content through it too, eventually replacing MarkdownUI. That's a separate PR.

## What This Is

Pika is a SwiftUI + Rust chat app that communicates over MLS-encrypted Nostr messages (kind 443 wrapper). Bots already have a CLI for sending text messages over the same encrypted channel, with support for different inner event kinds (text, reactions, typing indicators).

This plan adds a new inner kind: **hypernote** — markdown with JSX components that bots generate to create rich, interactive UI.

Most interactions are simple: the user picks an option, answers a question, submits a form. The response goes back to the bot as a structured chat message — like LLM tool-use responses. But when an action has real-world weight (authorizing a payment, publishing a post, voting in a poll), the app constructs and signs a Nostr event. The signed event is cryptographic proof — verifiable by anyone, forgeable by no one. Two tiers: chat for the common case, crypto when it matters.

## Parser: `hypernote-mdx` (this crate)

The parser is a pure Rust crate. No Zig dependency, no C FFI, no cross-compilation gymnastics. Pika adds it as a path dependency:

```toml
# In pika's rust/Cargo.toml
hypernote-mdx = { path = "../hypernote-mdx" }
```

This replaces the earlier `zig-mdx` prototype and the `feat/zig-mdx-rust-backend` branch. That branch's `build.rs` (Zig static lib, C ABI, iOS target mapping) is no longer needed.

### API

```rust
// Parse MDX source into AST
let ast = hypernote_mdx::parse(source);

// Serialize AST to JSON string (crosses UniFFI boundary to Swift)
let json = hypernote_mdx::serialize_tree(&ast);

// Render AST back to canonical MDX source
let source = hypernote_mdx::render(&ast);
```

### JSON AST Format

`serialize_tree()` returns a JSON object. This is the interface boundary — Swift receives this string via UniFFI and deserializes it into native Swift types for rendering.

```json
{
  "type": "root",
  "children": [
    { "type": "heading", "level": 1, "children": [{ "type": "text", "value": "Hello" }] },
    { "type": "paragraph", "children": [{ "type": "text", "value": "Some text" }] },
    {
      "type": "mdx_jsx_element",
      "name": "Card",
      "attributes": [{ "name": "title", "type": "literal", "value": "My Card" }],
      "children": [
        { "type": "mdx_jsx_self_closing", "name": "TextInput", "attributes": [
          { "name": "name", "type": "literal", "value": "message" },
          { "name": "placeholder", "type": "literal", "value": "Type here..." }
        ]}
      ]
    }
  ],
  "source": "...",
  "errors": []
}
```

### Node Types

| Type | Fields | Notes |
|------|--------|-------|
| `root` | `children`, `source`, `errors` | Top-level wrapper |
| `heading` | `level`, `children` | Level 1-6 |
| `paragraph` | `children` | Block of inline content |
| `text` | `value` | Raw text content |
| `strong` | `children` | Bold |
| `emphasis` | `children` | Italic |
| `code_inline` | `value` | Inline code |
| `code_block` | `value`, `lang?` | Fenced code block |
| `link` | `url`, `children` | Hyperlink |
| `image` | `url`, `children` | Image (children = alt text) |
| `blockquote` | `children` | Block quote |
| `list_unordered` | `children` | Bullet list |
| `list_ordered` | `children` | Numbered list |
| `list_item` | `children` | List item |
| `hr` | — | Horizontal rule |
| `hard_break` | — | Explicit line break |
| `mdx_jsx_element` | `name`, `attributes`, `children` | `<Card>...</Card>` |
| `mdx_jsx_self_closing` | `name`, `attributes` | `<TextInput />` |
| `mdx_jsx_fragment` | `children` | `<>...</>` |
| `mdx_text_expression` | `value` | `{form.name}` inline |
| `mdx_flow_expression` | `value` | `{expression}` block-level |
| `frontmatter` | `format`, `value` | `format` is `"yaml"` or `"json"` |

### JSX Attributes

Each attribute has `name`, `type`, and optional `value`:

```json
{ "name": "action", "type": "literal", "value": "approve" }
{ "name": "data", "type": "expression", "value": "form.message" }
```

`type` is `"literal"` (string value) or `"expression"` (dynamic `{...}` value).

### Frontmatter

Both YAML and JSON frontmatter are supported:

- **YAML** (`---\n...\n---`) — standard markdown frontmatter
- **JSON** (`` ```hnmd\n{...}\n``` ``) — for `.hnmd` files with action definitions

The parser stores the raw string in `value` without interpreting it. Over Nostr events, there is no frontmatter — metadata lives in event tags. Frontmatter is only for `.hnmd` files on disk.

## The Format

### In a Nostr event (over MLS)

Hypernote messages use a new inner event kind inside the existing kind 443 MLS wrapper. Pika's message handling already switches on inner kind for text, reactions, and typing indicators — this is one more case.

**Content field:** Pure MDX. No fencing, no metadata, no JSON envelope. Just markdown and JSX.

```
# Invoice from @merchant

**Service:** API hosting (June 2026)

<Card>
  <Caption>Amount</Caption>
  <Heading>50,000 sats</Heading>
  <Caption>Expires in 12 minutes</Caption>
</Card>

<HStack gap="4">
  <SubmitButton action="reject" variant="secondary">Reject</SubmitButton>
  <SubmitButton action="approve" variant="danger">Authorize Payment</SubmitButton>
</HStack>
```

**Tags:** All metadata lives in tags. Action definitions use JSON-in-tag:

```json
["actions", "{\"approve\":{\"kind\":21121,\"content\":\"\",\"tags\":[[\"p\",\"bot-pubkey-hex\"],[\"invoice\",\"lnbc500u1pj9nrzy...\"],[\"amount\",\"50000\"],[\"memo\",\"API hosting — June 2026\"],[\"authorization\",\"single-use-payment\"]],\"confirm\":\"Pay 50,000 sats for API hosting?\"},\"reject\":{\"kind\":21122,\"content\":\"\",\"tags\":[[\"p\",\"bot-pubkey-hex\"],[\"invoice\",\"lnbc500u1pj9nrzy...\"],[\"status\",\"rejected\"]]}}"]
```

Simple metadata uses plain tags:

```json
["title", "Payment Authorization"]
```

### As a .hnmd file (optional)

For portability and hand-editing, `.hnmd` files can include a JSON headmatter block. This is a convenience for passing hypernotes around outside of Nostr events — the headmatter maps to what would otherwise be event tags.

````
```hnmd
{
  "title": "Payment Authorization",
  "actions": {
    "approve": {
      "kind": 21121,
      "tags": [["p", "{{bot.pubkey}}"], ["invoice", "lnbc500u1pj9nrzy..."]],
      "confirm": "Pay 50,000 sats for API hosting?"
    }
  }
}
```

# Invoice from @merchant

<Card>
  <Heading>50,000 sats</Heading>
</Card>
````

The headmatter is not part of the Nostr event format. It exists only for files.

### Only hypernote-kind messages go through the renderer

Only messages with the hypernote inner kind are parsed by `hypernote-mdx`. Regular `Kind::ChatMessage` messages continue using the existing MarkdownUI renderer in Swift. Plain markdown within a hypernote is just the subset of MDX with no JSX components — a hypernote that says `**hello**` renders with bold text, and one with `<Card>` renders a card. The hypernote renderer handles the full spectrum from plain text to rich interactive UI, but only for messages explicitly sent as hypernote kind.

## Architecture

Four layers. Three in Rust, one in Swift.

```
Bot sends MLS message (kind 443 wrapper, hypernote inner kind)
  │
  ▼
┌─────────────────────────────────────────┐
│  RUST                                   │
│                                         │
│  Layer 1: PARSE (this crate)            │
│  hypernote_mdx::parse() → AST           │
│  hypernote_mdx::serialize_tree() → JSON  │
│  Extract action defs from tags → struct  │
│                                         │
│  Layer 2: SCOPE                         │
│  Simple dot-path evaluation             │
│  (form.fieldName, bot.pubkey)           │
│                                         │
│  Layer 3: ACTIONS                       │
│  Tier 1: Chat action → MLS reply        │
│  Tier 2: Signed action → Nostr event    │
│                                         │
└──────────────────┬──────────────────────┘
                   │ AST JSON string (via UniFFI)
                   ▼
┌─────────────────────────────────────────┐
│  SWIFT                                  │
│                                         │
│  Layer 4: RENDER                        │
│  Walk AST JSON → SwiftUI views          │
│  Local @State for form inputs           │
│  On submit: dispatch {action, form} to  │
│  Rust                                   │
│                                         │
└─────────────────────────────────────────┘
```

### Layer 1: Parse (Rust — this crate)

`hypernote-mdx` handles tokenization, parsing, AST construction, and JSON serialization. It's the only layer currently implemented.

The integration point is `rust/src/core/storage.rs` around line 380, where MDK messages are converted to `ChatMessage` structs. Parsing only runs for hypernote-kind messages (see Transport section below for the exact code).

**Additions to Pika's Rust layer:**

Add to `rust/src/state.rs`:

```rust
/// New struct — add to state.rs alongside existing ChatMessage
#[derive(Debug, Clone, uniffi::Record)]
pub struct HypernoteData {
    pub ast_json: String,                        // JSON AST from hypernote_mdx
    pub actions: Option<String>,                 // Raw JSON of action definitions (if any)
    pub title: Option<String>,                   // From tags
}
```

Add the `hypernote` field to the existing `ChatMessage` struct in `rust/src/state.rs:291`:

```rust
// Existing struct (rust/src/state.rs:291-307) — add one field
#[derive(uniffi::Record, Clone, Debug)]
pub struct ChatMessage {
    pub id: String,
    pub sender_pubkey: String,
    pub sender_name: Option<String>,
    pub content: String,                         // Raw content (MDX source for hypernotes)
    pub display_content: String,                 // Mentions resolved
    pub reply_to_message_id: Option<String>,
    pub mentions: Vec<Mention>,
    pub timestamp: i64,
    pub is_mine: bool,
    pub delivery: MessageDeliveryState,
    pub reactions: Vec<ReactionSummary>,
    pub media: Vec<ChatMediaAttachment>,
    pub poll_tally: Vec<PollTally>,
    pub my_poll_vote: Option<String>,
    pub html_state: Option<String>,
    pub hypernote: Option<HypernoteData>,        // NEW — parsed hypernote data (if hypernote kind)
}
```

Add the kind constant to `rust/src/core/mod.rs` near the existing kind constants (line 104-105):

```rust
// Existing constants (rust/src/core/mod.rs:104-105)
const TYPING_INDICATOR_KIND: Kind = Kind::Custom(20_067);
pub(crate) const CALL_SIGNAL_KIND: Kind = Kind::Custom(10);

// Add:
pub(crate) const HYPERNOTE_KIND: Kind = Kind::Custom(9467);
```

**Action definition** (used in Layer 3, not UniFFI-exported — internal Rust only):

```rust
/// Action definition (parsed from ["actions", "{...}"] tag)
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ActionDef {
    pub kind: u16,                               // Nostr event kind to publish
    pub content: Option<String>,                 // Template: "{{form.message}}"
    pub tags: Option<Vec<Vec<String>>>,          // May contain {{}} templates
    pub confirm: Option<String>,                 // Confirmation prompt text
}
```

### Layer 2: Scope (Rust)

Minimal expression evaluator for template interpolation in action definitions. v1 supports only simple dot-path resolution:

- `form.fieldName` → value from the form dict Swift sends on action submit
- `bot.pubkey` → the bot's pubkey (the message sender)
- `user.pubkey` → the current user's pubkey

No pipe filters, no defaults operator, no complex expressions. These can be added later.

```rust
fn interpolate_template(
    template: &str,
    form: &HashMap<String, String>,
    bot_pubkey: &str,
    user_pubkey: &str,
) -> String {
    // Replace all {{path}} occurrences with resolved values
    // e.g. "{{form.message}}" → "Hello world"
    // e.g. "{{bot.pubkey}}" → "abcd1234..."
    let re = regex::Regex::new(r"\{\{(\w+)\.(\w+)\}\}").unwrap();
    re.replace_all(template, |caps: &regex::Captures| {
        match (&caps[1], &caps[2]) {
            ("form", field) => form.get(field).cloned().unwrap_or_default(),
            ("bot", "pubkey") => bot_pubkey.to_string(),
            ("user", "pubkey") => user_pubkey.to_string(),
            _ => String::new(),
        }
    }).to_string()
}
```

### Layer 3: Actions (Rust)

There are two tiers of actions. Most bot interactions are conversational — the user picks an option, answers a question, fills in a value. These don't need cryptography. Only actions with real-world weight (signing a payment, publishing a post, voting in a poll) produce signed Nostr events.

#### Tier 1: Chat actions (the common case)

The user taps a button or submits a form. Pika sends a structured JSON reply back to the bot over the existing MLS chat — the same way LLM tool-use responses work. No Nostr event, no signing, no relay publishing. Just a message the bot can parse.

```
User taps "Option B"
  → Pika sends chat message: {"action": "choose", "value": "B"}
  → Bot receives it as a tool result and continues the conversation
```

Chat actions are the default. If an action name is referenced by a `SubmitButton` but has no corresponding entry in the `["actions", "..."]` tag, it's a chat action. Rust collects the form data, wraps it as `{"action": "<name>", "form": {...}}`, and sends it as a **regular text message** (same inner kind as normal chat) back to the bot over MLS.

This covers:
- Multiple-choice questions ("Which model? A, B, or C")
- Form submissions ("What's your name?")
- Confirmations ("Ready to proceed? Yes / No")
- Any interaction where the bot is the only audience

Add to `rust/src/actions.rs` (the `AppAction` enum is `#[derive(uniffi::Enum)]` so Swift gets it automatically):

```rust
// In AppAction enum (rust/src/actions.rs:4), add under "// Chat management" section:
HypernoteAction {
    chat_id: String,
    message_id: String,         // Which hypernote message triggered this
    action_name: String,
    form: HashMap<String, String>,
}
```

Add the tag match to `AppAction::tag()` (line 166):

```rust
AppAction::HypernoteAction { .. } => "HypernoteAction",
```

The handler in `rust/src/core/mod.rs` checks whether the action has a definition (signed) or not (chat):

```rust
AppAction::HypernoteAction { chat_id, message_id, action_name, form } => {
    // Look up the HypernoteData for this message from current chat state
    // Check if action_name exists in the parsed actions JSON
    // If no definition → chat action: serialize {"action": name, "form": {...}}
    //   and send via self.publish_chat_message_with_tags() with Kind::ChatMessage
    // If definition found → signed action: interpolate, build event, sign, publish
}
```

**Note:** `publish_chat_message_with_tags()` is defined in `rust/src/core/chat_media.rs:456` and already accepts a `Kind` parameter — it can be reused directly for sending chat action replies as `Kind::ChatMessage`.

#### Tier 2: Signed actions (when it matters)

When the `["actions", "..."]` tag defines an action with a `kind`, it's a signed action. Rust constructs a real Nostr event, signs it with the user's key, and publishes it to relays. The signed event is cryptographic proof — verifiable by anyone, forgeable by no one.

```
User taps "Authorize Payment"
  → Pika looks up action def: kind 21121, tags with invoice details
  → Interpolates {{form.amount}} etc.
  → Signs Nostr event with user's key
  → Publishes to relays
  → Bot (or any third party) can verify the signature
```

This is for:
- Payments and financial authorizations
- Publishing content to Nostr (posts, reactions, polls)
- Any action where a third party needs to verify the user's intent

Both tiers use a single `AppAction::HypernoteAction` variant (see Tier 1 above). The dispatch logic in the handler determines which tier applies:

**Dispatch logic:**

When Swift dispatches `HypernoteAction`, Rust:

1. Finds the `HypernoteData` for the message (look up by `message_id` in the current `ChatViewState.messages`)
2. Parses the `actions` JSON string if present
3. Checks if `action_name` exists in the parsed actions map
4. **If no definition found** → chat action. Serialize `{"action": "<name>", "form": {...}}`, send as `Kind::ChatMessage` via `self.publish_chat_message_with_tags()`
5. **If definition found** → signed action:
   a. Deserialize the `ActionDef`
   b. Interpolate `{{...}}` templates in content and tags
   c. Construct a Nostr event using `EventBuilder::new(Kind::from(action_def.kind), content).tags(tags)`
   d. Sign with the user's key (available via `sess.keys` in the session)
   e. Publish to relays via `sess.client.send_event_to()`

Buttons are not disabled after use. Actions are idempotent — if a user taps twice, two responses are sent. The bot or receiving service handles deduplication.

### Layer 4: Render (Swift)

A recursive SwiftUI view builder that walks the AST JSON. This is a fresh implementation — a new `HypernoteRenderer` view that lives alongside the existing rendering in `MessageBubbleViews.swift`.

**Existing rendering context:** Messages currently flow through `parseMessageSegments()` (line 36) which splits content into `.markdown(String)` / `.pikaPrompt(PikaPrompt)` / `.pikaHtml(...)` segments. Markdown segments are rendered with the `MarkdownUI` library (`Markdown(text).markdownTheme(...)` at lines 534, 554). The hypernote renderer is a separate branch that replaces this entire pipeline for hypernote-kind messages only. The `MarkdownUI` dependency and existing rendering stay for regular messages.

**Integration point in MessageBubble:** Around line 530-546 in `MessageBubbleViews.swift`, where `hasText` triggers the segment rendering loop, add a check:

```swift
if let hypernote = message.hypernote {
    HypernoteRenderer(astJson: hypernote.astJson, actions: hypernote.actions, ...)
} else if hasText {
    // Existing ForEach(segments) { ... Markdown(text) ... } path
}
```

Swift receives the AST JSON string from `HypernoteData.ast_json` via UniFFI (auto-generated from the `#[derive(uniffi::Record)]` struct). Deserialize into Swift types:

```swift
struct AstNode: Decodable {
    let type: String
    let value: String?
    let level: Int?
    let name: String?
    let url: String?
    let lang: String?
    let format: String?
    let attributes: [AstAttribute]?
    let children: [AstNode]?
}

struct AstAttribute: Decodable {
    let name: String
    let type: String          // "literal" or "expression"
    let value: String?
}
```

Render recursively:

```swift
@ViewBuilder
func renderNode(_ node: AstNode, form: Binding<[String: String]>, onAction: @escaping (String) -> Void) -> some View {
    switch node.type {
    case "heading":
        // node.level determines font
    case "paragraph":
        // Render children inline
    case "text":
        Text(node.value ?? "")
    case "strong":
        // Render children with .bold()
    case "mdx_jsx_element", "mdx_jsx_self_closing":
        renderComponent(node, form: form, onAction: onAction)
    // ... etc
    default:
        EmptyView()
    }
}
```

**Markdown nodes → SwiftUI:**

| AST Node | SwiftUI |
|----------|---------|
| `heading` (1-6) | `Text().font(.title/.title2/.title3/...)` |
| `paragraph` | `Text` with inline children |
| `text` | `Text(value)` |
| `strong` | `.bold()` |
| `emphasis` | `.italic()` |
| `code_inline` | `.font(.system(.body, design: .monospaced))` |
| `code_block` | Monospace `Text` with background |
| `link` | `Link` or `Text` with `.underline()` |
| `image` | `AsyncImage` |
| `list_ordered/unordered` | `VStack` with bullets/numbers |
| `blockquote` | Styled with leading border |
| `hr` | `Divider()` |

**JSX components** map to the component catalog (see below).

**Unknown components** render their children with a subtle visual indicator (light dashed border or dimmed style) so content is never lost but it's visible that something wasn't recognized.

**Form state** is local `@State` / `@Observable` in Swift, scoped to the message view. TextInput binds to a local `[String: String]` dict keyed by the input's `name` attribute. When a SubmitButton is tapped, Swift dispatches the action name + form dict to Rust:

```swift
// In the message view
@State private var formState: [String: String] = [:]

// When SubmitButton tapped — single action, Rust decides chat vs signed:
manager.dispatch(.hypernoteAction(
    chatId: chat.id,
    messageId: message.id,
    actionName: "choose",
    form: formState
))
```

## Component Catalog

The catalog is not a fixed spec. The app supports a set of components. The bot operator describes available components in the LLM's system prompt. The LLM generates MDX using only what it was told about. Unknown components degrade gracefully.

### Starting catalog for Pika

**Layout:**

| Component | Props | SwiftUI | Notes |
|-----------|-------|---------|-------|
| `Card` | `title?` | `GroupBox` or styled `VStack` | Visual container with boundary |
| `VStack` | `gap?` | `VStack(spacing:)` | Vertical layout |
| `HStack` | `gap?` | `HStack(spacing:)` | Horizontal layout |

**Content:**

| Component | Props | SwiftUI | Notes |
|-----------|-------|---------|-------|
| `Heading` | `level? (1-3)` | `Text().font(.title/.title2/.title3)` | Section heading |
| `Body` | — | `Text().font(.body)` | Paragraph text |
| `Caption` | — | `Text().font(.caption).foregroundStyle(.secondary)` | Muted text |

**Nostr Data:**

| Component | Props | SwiftUI | Notes |
|-----------|-------|---------|-------|
| `Profile` | `pubkey` | Custom view (avatar + name) | Lazy-fetched via Pika's existing relay infra |
| `Note` | `id` | Custom view (note content) | Lazy-fetched via Pika's existing relay infra |

**Interactive:**

| Component | Props | SwiftUI | Notes |
|-----------|-------|---------|-------|
| `TextInput` | `name`, `placeholder?` | `TextField` | Binds to local form state by `name` |
| `SubmitButton` | `action`, `variant?` | `Button` | Chat action (default) or signed action if defined in tags |

**SubmitButton variants** communicate intent, not color:
- `primary` (default) → `.borderedProminent`
- `secondary` → `.bordered`
- `danger` → `.borderedProminent` + destructive role

### Nostr embeds

`<Profile pubkey="npub1..."/>` and `<Note id="note1..."/>` use Pika's existing lazy-fetch pattern:

1. Swift renders a placeholder (loading indicator)
2. Dispatches a fetch request to Rust (new `AppAction` variant)
3. Rust fetches the profile/event from relays (using the existing profile cache and relay pool)
4. Rust emits a state update with the resolved data
5. Swift re-renders with the actual content

This is consistent with how Pika already handles profile lookups for chat participants.

## Transport

Hypernote messages use the existing MLS-encrypted transport. No new relay connections, no new encryption schemes.

```
Bot CLI / Daemon
  │
  │  new daemon command: send_hypernote
  │  fields: nostr_group_id, content (MDX string),
  │          actions (optional JSON string), title (optional)
  │
  ▼
Kind 443 MLS wrapper
  └── Inner event: Kind::Custom(HYPERNOTE_KIND)
        ├── content: raw MDX
        ├── tags: [["actions", "{...}"], ["title", "..."], ...]
        └── (same MLS group, same relays, same everything)
```

**Bot CLI / daemon integration:**

The daemon (`crates/pikachat-sidecar/src/daemon.rs`) processes commands via a JSON `InCmd` enum (line 38). Add a new variant alongside `SendMessage`:

```rust
// In InCmd enum (daemon.rs:40), add alongside SendMessage (line 65):
SendHypernote {
    #[serde(default)]
    request_id: Option<String>,
    nostr_group_id: String,
    content: String,                    // MDX content
    #[serde(default)]
    actions: Option<String>,            // JSON string of action definitions
    #[serde(default)]
    title: Option<String>,
},
```

The handler follows the `SendMessage` pattern (line 2667) — construct a rumor with the hypernote kind + tags, then call `sign_and_publish()`:

```rust
InCmd::SendHypernote { request_id, nostr_group_id, content, actions, title } => {
    let mls_group_id = match resolve_group(&mdk, &nostr_group_id) { ... };
    let mut tags = Vec::new();
    if let Some(actions_json) = actions {
        tags.push(Tag::custom(TagKind::custom("actions"), [actions_json]));
    }
    if let Some(t) = title {
        tags.push(Tag::custom(TagKind::custom("title"), [t]));
    }
    let rumor = EventBuilder::new(Kind::Custom(HYPERNOTE_KIND), content)
        .tags(tags)
        .build(keys.public_key());
    match sign_and_publish(&client, &relay_urls, &mdk, &keys, &mls_group_id, rumor, "daemon_send_hypernote").await {
        Ok(ev) => { out_tx.send(out_ok(request_id, Some(json!({"event_id": ev.id.to_hex()})))).ok(); }
        Err(e) => { out_tx.send(out_error(request_id, "publish_failed", format!("{e:#}"))).ok(); }
    }
}
```

The CLI (`cli/src/main.rs`) can also get a `send-hypernote` subcommand that mirrors the existing `Send` command (line 1126), using the same `EventBuilder` pattern but with `Kind::Custom(HYPERNOTE_KIND)` + tags.

**Pika message handling:**

In `rust/src/core/mod.rs` (around line 3124), where inner events are processed via `MessageProcessingResult::ApplicationMessage`, add a match arm for the hypernote kind. The existing match looks like:

```rust
// rust/src/core/mod.rs:3124 — current code
match msg.kind {
    Kind::Custom(20_067) if msg.content == "typing" && /* d-tag check */ => {
        is_typing_indicator = true;
        app_sender = Some(msg.pubkey);
    }
    Kind::Custom(10) => {
        is_call_signal_kind = true;
        // ...
    }
    Kind::ChatMessage | Kind::Reaction => {
        if msg.kind == Kind::Reaction { is_reaction = true; }
        app_sender = Some(msg.pubkey);
        app_content = Some(msg.content.clone());
    }
    kind => {
        tracing::warn!(?kind, "ignoring app message with unknown kind");
        return;
    }
}
```

Add the hypernote kind to the `Kind::ChatMessage | Kind::Reaction` arm (or as a separate arm that sets a flag):

```rust
Kind::ChatMessage | Kind::Reaction | Kind::Custom(HYPERNOTE_KIND) => {
    if msg.kind == Kind::Reaction { is_reaction = true; }
    if msg.kind == Kind::Custom(HYPERNOTE_KIND) { is_hypernote = true; }
    app_sender = Some(msg.pubkey);
    app_content = Some(msg.content.clone());
}
```

In `rust/src/core/storage.rs` (around line 301), the visibility filter **must also include the hypernote kind** or hypernotes are silently dropped:

```rust
// Current: only ChatMessage and Reaction are visible
.filter(|m| m.kind == Kind::ChatMessage || m.kind == Kind::Reaction)

// Updated: include hypernote
.filter(|m| m.kind == Kind::ChatMessage || m.kind == Kind::Reaction || m.kind == Kind::Custom(HYPERNOTE_KIND))
```

In `rust/src/core/storage.rs` (around line 380), where `ChatMessage` structs are built, detect hypernote kind and populate the field:

```rust
ChatMessage {
    id,
    sender_pubkey: sender_hex,
    sender_name,
    content: m.content.clone(),
    display_content,
    // ... existing fields ...
    hypernote: if m.kind == Kind::Custom(HYPERNOTE_KIND) {
        let ast_json = hypernote_mdx::serialize_tree(&hypernote_mdx::parse(&m.content));
        let actions = m.tags.iter()
            .find(|t| t.kind() == TagKind::custom("actions"))
            .and_then(|t| t.content().map(|s| s.to_string()));
        let title = m.tags.iter()
            .find(|t| t.kind() == TagKind::custom("title"))
            .and_then(|t| t.content().map(|s| s.to_string()));
        Some(HypernoteData { ast_json, actions, title })
    } else {
        None
    },
}
```

In `ios/Sources/Views/MessageBubbleViews.swift`, check `message.hypernote` — if present, render with the hypernote renderer instead of the `parseMessageSegments()` + MarkdownUI path. The existing rendering happens around line 530-546 where `ForEach(segments)` iterates markdown/prompt/html segments.

## Implementation Checklist

### Rust core changes

- [x] Add `hypernote-mdx` as workspace dependency in `rust/Cargo.toml` — `Kind::Custom(9467)`
- [x] Pick an inner kind number for hypernote — add `const HYPERNOTE_KIND` to `rust/src/core/mod.rs`
- [x] Define `HypernoteData` struct with `#[derive(uniffi::Record)]` in `rust/src/state.rs`
- [x] Add `hypernote: Option<HypernoteData>` field to `ChatMessage` in `rust/src/state.rs`
- [x] Add match arm for `Kind::Custom(9467)` in `rust/src/core/mod.rs`
- [x] Add hypernote kind to visibility filter in `rust/src/core/storage.rs`
- [x] Parse MDX + extract tags when building `ChatMessage` in `rust/src/core/storage.rs`
- [x] Add `AppAction::HypernoteAction` variant to `rust/src/actions.rs` + tag match
- [x] Add handler for `HypernoteAction` in `rust/src/core/mod.rs` — dispatch chat vs signed
- [x] Implement `interpolate_template()` for `{{dot.path}}` resolution (inline in handler, no regex dep)

### Swift changes (`ios/`)

- [x] Add `HypernoteAstNode` / `HypernoteAstAttribute` Decodable types in `ios/Sources/Views/HypernoteRenderer.swift`
- [x] Build `HypernoteRenderer` view — recursive AST walker producing SwiftUI
- [x] Implement markdown node rendering (heading, paragraph, text, strong, emphasis, code, link, image, list, blockquote, hr, hard_break)
- [x] Implement component catalog (Card, VStack, HStack, Heading, Body, Caption, TextInput, SubmitButton)
- [x] Unknown component fallback — render children with subtle dashed border
- [x] Local `@State` form dict per message, bound to TextInput by `name`
- [x] SubmitButton dispatches `AppAction.hypernoteAction` via `onHypernoteAction` callback chain
- [x] In `MessageBubbleViews.swift`, branch on `message.hypernote != nil` to use `HypernoteRenderer`

### Bot CLI / daemon changes

- [x] Add `SendHypernote` variant to `InCmd` enum in `crates/pikachat-sidecar/src/daemon.rs`
- [x] Add handler — construct rumor with `Kind::Custom(9467)` + action/title tags, call `sign_and_publish()`
- [x] Add `send-hypernote` subcommand to `cli/src/main.rs` (mirrors existing `Send`)
- [x] Accept `--content` (MDX string), `--actions` (JSON string), `--title`

## Demo Scenarios

### Demo 1: Chat action (simple — no crypto)

Bot sends a message with no action tags — just MDX content:

```
Which language should I use for the backend?

<HStack gap="4">
  <SubmitButton action="choose" variant="secondary">Rust</SubmitButton>
  <SubmitButton action="choose" variant="secondary">Go</SubmitButton>
  <SubmitButton action="choose" variant="secondary">TypeScript</SubmitButton>
</HStack>
```

User taps "Rust". Pika sends `{"action": "choose", "form": {}}` as a regular text message back to the bot. The bot sees the tool result and continues the conversation. No signing, no relays. Just chat.

### Demo 2: Signed action (crypto)

Bot sends MDX with an action tag:

```
# Quick Note

<Card>
  <Caption>Post to Nostr</Caption>
  <TextInput name="message" placeholder="What's on your mind?" />
  <SubmitButton action="post" variant="primary">Publish</SubmitButton>
</Card>
```

With action tag:

```json
["actions", "{\"post\":{\"kind\":1,\"content\":\"{{form.message}}\",\"tags\":[]}}"]
```

User types a message, taps Publish. Pika sees that `post` has an `ActionDef` with `kind: 1`. It constructs a kind 1 Nostr event with the message as content, signs it with the user's key, publishes to relays. A real Nostr post, created from bot-generated UI.

### Demo 3: Plain markdown (no components)

Bot sends a regular message:

```
Here's what I found:

**3 matching results:**

1. nostr-sdk — Rust library for Nostr
2. nostr-tools — JavaScript library
3. python-nostr — Python library

Let me know which one to investigate.
```

Because the bot sent this as a hypernote kind, it renders with the hypernote renderer — rich formatting (bold, numbered list) even though there are no JSX components. The hypernote renderer handles the full spectrum from plain markdown to interactive UI. (If the bot had sent this as a regular `Kind::ChatMessage`, it would render via the existing MarkdownUI path instead — same visual result, different code path.)

## Backward Compatibility

**Old clients** (Pika versions without hypernote support) will hit the `kind => { tracing::warn!(...); return; }` fallthrough in `mod.rs:3147` and silently ignore hypernote messages. This is acceptable for the bot-only initial launch — bots send hypernotes into group chats where all participants are expected to have a recent client.

If needed later, bots could send a fallback `Kind::ChatMessage` alongside the hypernote (or include a `["fallback", "plain text summary"]` tag that old clients could display). This is not required for v1.

## Future Milestones (Not In Scope Now)

- **Convergence (Approach B):** Parse `Kind::ChatMessage` content through `hypernote-mdx` too, replace MarkdownUI with the hypernote renderer for all messages, migrate `pika-prompt` and `pika-html` to JSX components
- **Richer expression evaluation:** pipe filters (`| format_date`), defaults (`// 'Anon'`), conditionals
- **Confirmation flows:** biometric auth for high-stakes actions (payments, transfers)
- **Nostr queries in tags:** let the bot declare relay subscriptions so the UI shows live data
- **Each/ForEach component:** iterate over lists of data
- **Select/Option components:** dropdowns and pickers
- **NumberInput component:** numeric input with keyboard type
- **Live-updating UI:** bot updates a previous message via replaceable events
- **Custom component registration:** bot declares new component types at runtime
- **Theming:** visual customization layer (the semantic model makes this additive)
