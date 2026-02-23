# Reliable MoQ Chat Profile (MCR-00)

Status: Draft  
Last updated: 2026-02-23  
Audience: Relay/backend and client implementers building reliable Marmot chat delivery over MoQ.

## 1. Scope

This document specifies a reliability profile for JSON chat messages transported over MoQ.

This profile intentionally does not define multi-relay quorum acknowledgement.

## 2. Conventions and Terminology

The key words `MUST`, `MUST NOT`, `REQUIRED`, `SHOULD`, `SHOULD NOT`, and `MAY` are to be interpreted as described in RFC 2119 and RFC 8174.

Terms:

1. `room_id`: stable room/conversation identifier.
2. `msg_id`: sender-generated globally unique message identifier.
3. `seq`: relay-assigned monotonically increasing sequence number scoped to `room_id`.
4. `head_seq`: latest persisted `seq` known by relay for a room.

## 3. Design Goals

Implementations of MCR-00 MUST provide:

1. Lossless JSON payload handling.
2. Relay-persisted acknowledgement semantics.
3. Deterministic catch-up after disconnect.
4. Deterministic per-room ordering.

MCR-00 does not require exactly-once transport over network links. Instead, endpoints MUST provide exactly-once application behavior using sequence checks and deduplication.

## 4. Transport Requirements

1. Reliable chat delivery paths MUST use QUIC/WebTransport streams.
2. Datagrams MUST NOT be used for reliable chat messages.
3. Implementations MAY provide WebSocket fallback. If they do, they MUST treat fallback as a compatibility path and MUST NOT weaken persistence, ordering, dedupe, or replay semantics defined by this profile.

## 5. Delivery and Ordering Model

## 5.1 Delivery Guarantees

1. Publisher -> Relay delivery is at-least-once.
2. Relay -> Subscriber delivery is at-least-once.
3. Client applications MUST implement dedupe and sequence continuity checks to achieve exactly-once user-visible behavior.

## 5.2 Ordering Guarantees

1. Relay MUST assign strictly increasing `seq` values per `room_id`.
2. Subscribers MUST apply messages in ascending `seq`.
3. If a gap is detected, subscribers MUST run catch-up before applying higher `seq` messages.

## 5.3 Durability

1. Relay MUST durably append a message before issuing a success receipt.
2. A success receipt MUST include the assigned `seq`.
3. Clients MAY render optimistic pending UI before receipt, but MUST keep pending state until success receipt is received.

## 6. Message Envelope

## 6.1 Canonical JSON Shape

Relay-persisted and relay-delivered chat envelopes MUST conform to:

```json
{
  "v": 1,
  "room_id": "string",
  "seq": 12345,
  "msg_id": "01J...ULID",
  "sender_id": "npub1...",
  "sent_at_ms": 1760000000000,
  "payload_type": "marmot_app_event",
  "payload": { "..." : "..." },
  "meta": {
    "trace_id": "optional",
    "content_type": "application/json"
  }
}
```

## 6.2 Envelope Rules

1. Publish requests from clients MUST omit `seq`.
2. Relay success paths MUST include `seq`.
3. `payload` MUST be treated as opaque application data by generic relay transport components.
4. Receivers MUST ignore unknown top-level fields for forward compatibility.

## 7. Logical Track Layout

This profile defines logical room-scoped tracks:

1. `rooms/<room_id>/chat.live`: persisted envelopes in ascending `seq`.
2. `rooms/<room_id>/chat.receipts`: publish receipts.
3. `rooms/<room_id>/chat.control`: optional control stream.

If underlying transport behavior cannot guarantee in-order delivery for `chat.live`, implementations MUST enforce ordering in client logic using Section 10.

## 8. Required Relay HTTP Catch-up API

Because live-only subscriptions are insufficient for deterministic replay in current MoQ-lite deployments, relays implementing MCR-00 MUST expose both endpoints below.

## 8.1 Head Endpoint

`GET /mcr/v1/rooms/{room_id}/head`

Response MUST include the latest persisted `seq`:

```json
{
  "room_id": "abc",
  "head_seq": 1299
}
```

## 8.2 Range Endpoint

`GET /mcr/v1/rooms/{room_id}/messages?from_seq={u64}&limit={n}`

Response:

```json
{
  "room_id": "abc",
  "from_seq": 1200,
  "to_seq": 1299,
  "has_more": true,
  "items": [ { "...envelope..." } ]
}
```

Constraints:

1. `items` MUST be strictly ascending by `seq`.
2. Default `limit` MUST be 200 if omitted.
3. `limit` MUST NOT exceed 1000.
4. Empty `items` for `from_seq > head_seq` MUST be treated as valid.

## 9. Publish Procedure

## 9.1 Client Behavior

1. Client MUST generate a unique `msg_id`.
2. Client MUST send publish request without `seq`.
3. Client SHOULD include local send timestamp as `sent_at_ms`.

## 9.2 Relay Behavior

On publish request, relay MUST:

1. Validate authentication and authorization for `room_id`.
2. Validate schema and size limits.
3. Enforce configured rate limits.
4. Assign next `seq` for `room_id`.
5. Durably persist envelope.
6. Emit success receipt.
7. Fan out envelope on `chat.live`.

## 9.3 Receipt Schemas

Success:

```json
{
  "v": 1,
  "status": "PERSISTED",
  "room_id": "string",
  "msg_id": "string",
  "seq": 12345,
  "persisted_at_ms": 1760000001000
}
```

Failure:

```json
{
  "v": 1,
  "status": "REJECTED",
  "room_id": "string",
  "msg_id": "string",
  "code": "TOO_FAST",
  "reason": "rate limit exceeded"
}
```

## 10. Subscribe, Gap Detection, and Recovery

## 10.1 Initial Attach

Client MUST execute:

1. Load local `last_seq` for room.
2. Query `head_seq`.
3. If `last_seq < head_seq`, fetch `[last_seq + 1, head_seq]` via range endpoint.
4. Apply fetched envelopes in ascending `seq`.
5. Subscribe to `chat.live`.

## 10.2 Live Processing

For incoming envelope sequence `s`:

1. If `s == last_seq + 1`: apply and set `last_seq = s`.
2. If `s <= last_seq`: treat as duplicate and discard.
3. If `s > last_seq + 1`: mark gap and run catch-up from `last_seq + 1`; client MUST NOT commit higher `seq` messages until gap is reconciled.

## 10.3 Reconnect

After any reconnect, client MUST repeat Section 10.1 before resuming normal live apply.

## 11. Dedupe and Idempotency

Clients MUST maintain:

1. `last_seq_by_room`.
2. `seen_msg_ids` in a bounded retention window.

Clients MUST NOT render a duplicate `msg_id` more than once.

Relays SHOULD enforce uniqueness of (`room_id`, `msg_id`) and SHOULD return `DUPLICATE` for repeated publish attempts.

## 12. Error Codes

MCR-00 defines the following error/result codes:

1. `SUCCESS`
2. `ACCEPTED`
3. `DUPLICATE`
4. `NOT_FOUND`
5. `REQUIRES_AUTHENTICATION`
6. `UNAUTHORIZED`
7. `INVALID`
8. `TOO_OPEN`
9. `TOO_LARGE`
10. `TOO_FAST`
11. `SHUTTING_DOWN`
12. `TEMPORARY_ERROR`
13. `PERSISTENT_ERROR`

Relay responses and receipts MUST use one of these values.

Clients receiving unknown codes SHOULD treat them as `PERSISTENT_ERROR`.

## 13. Security Requirements

1. Transport MUST provide authenticated encryption (TLS over QUIC/WebTransport).
2. Relay MUST authorize room access before persist or delivery.
3. Relay MUST NOT mutate `payload` content.
4. Identity/integrity of inner Marmot payloads remains the responsibility of Marmot/MLS layers.

## 14. Relay Storage Requirements

For each room, relay MUST maintain:

1. An append-only log keyed by `seq`.
2. A uniqueness index for `msg_id`.

Relay SHOULD expose configurable retention (default 30 days).

If retention deletes old data, `seq` monotonicity MUST still hold for remaining/new data, and old fetch ranges MAY return `NOT_FOUND`.

## 15. Backpressure and Limits

1. Relay SHOULD enforce per-identity publish rate limits.
2. On rate-limit rejection, relay MUST return `TOO_FAST`.
3. Relay SHOULD include `retry_after_ms` on `TOO_FAST`.
4. Relay MUST bound maximum message size and return `TOO_LARGE` when exceeded.

## 16. Compatibility with Current MoQ-lite

MCR-00 implementations targeting current MoQ-lite stacks MUST treat reliability as an application-layer contract:

1. Live stream for low-latency updates.
2. HTTP head/range API for replay and gap repair.
3. Client-enforced ordering and dedupe.

Implementations MUST NOT assume that raw subscription behavior alone provides full historical reliability.

## 17. Marmot Integration

1. Existing Marmot inner event formats MUST remain unchanged.
2. "ACK before local apply" rules for sensitive state transitions MUST be preserved.
3. For ordinary chat UX, optimistic rendering MAY be used, but committed state MUST depend on success receipt.

## 18. Compliance Checklist

A relay implementation is MCR-00 compliant if and only if it:

1. Implements Sections 4 through 16 requirements.
2. Emits success receipts only after durable append.
3. Supports head and range endpoints with required ordering constraints.

A client implementation is MCR-00 compliant if and only if it:

1. Implements Section 10 recovery behavior.
2. Implements Section 11 dedupe/idempotency behavior.
3. Never applies out-of-order messages across unresolved gaps.

## 19. Verification Criteria

Required test outcomes:

1. No user-visible message loss after 5-minute disconnect at 50 msg/s load.
2. Duplicate deliveries do not render duplicate chat items.
3. Packet loss/out-of-order conditions reconcile to contiguous applied `seq`.
4. Invalid/auth/rate-limit publish attempts return deterministic error codes.
5. Reconnect catch-up for 1000 messages completes within target SLO (recommended: <2s on local test net).

## 20. Implementation Order (Recommended)

1. Relay durable log, sequence allocator, and receipt path.
2. Head/range HTTP catch-up endpoints.
3. Live MoQ publish/subscribe wiring.
4. Client gap repair, dedupe, and pending/committed UI transitions.
