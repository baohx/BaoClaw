# Telegram Gateway v2 Migration Plan

<!-- rumdl-disable MD013 -->

## Goal

Migrate `baoclaw-telegram` from `node-telegram-bot-api` `0.66.x` to `2.1.x` without losing polling, message formatting, media/document handling, daemon streaming, command handling, graceful shutdown, or proxy support.

## Research Findings

`node-telegram-bot-api` v2 is a full rewrite, not a compatibility release. The old `TelegramBot` event-emitter semantics are removed; v2's `on` method is a middleware filter. v2 uses:

- `Bot` instead of the default `TelegramBot` class.
- `bot.api.<method>({ ...params })` with one parameter object per API call.
- Koa-style middleware/context dispatch instead of `bot.on(...)` and `bot.onText(...)`.
- `run(bot)` from the Node entrypoint for polling, with `bot.stop()` and
  `bot.isRunning()` for lifecycle control.
- `fromPath()` and `InputFile` from `node-telegram-bot-api/node` for uploads.
- Named, flat TypeScript exports supplied by the package; `@types/node-telegram-bot-api` must be removed.
- Node.js 18+ support; the repository's Node 24 runtime satisfies this requirement.

Reference: [v2 migration guide and changelog](https://github.com/yagop/node-telegram-bot-api/blob/master/CHANGELOG.md).

## Current Gateway Surface

The main migration target is `baoclaw-telegram/src/gateway.ts`:

- `gateway.ts:10` imports the v1 default `TelegramBot` class.
- `gateway.ts:443` constructs the bot with `{ polling: true }`.
- `gateway.ts:464` listens for `polling_error`.
- `gateway.ts:491` consumes daemon `stream/event` notifications and sends assistant/tool output.
- `gateway.ts:505`, `522`, `531`, `630`, `651`, and `1161` use `sendMessage`.
- `gateway.ts:566`, `612`, and `664` use `sendPhoto`.
- `gateway.ts:919` uses `sendDocument`.
- `gateway.ts:1149` uses `sendChatAction`.
- `gateway.ts:1189` registers the main `message` handler.
- `gateway.ts:1207` and `1261` use `getFileLink` for inbound files.
- `gateway.ts:683`, `832`, and `1316` call `stopPolling` during shutdown/error paths.
- Command routing is currently implemented through `COMMAND_REGISTRY` and the message handler, so it should remain a single middleware path rather than be duplicated.

## Proposed Design

Keep the daemon IPC layer and business logic unchanged. Replace only the Telegram transport boundary:

1. Import `Bot`, `type Message`, `type Update`, and `InputFile` from `node-telegram-bot-api`.
2. Import `fromPath` and `run` from `node-telegram-bot-api/node`.
3. Create `const bot = new Bot(config.token, { fetch })` when proxy support is configured; otherwise use the default constructor.
4. Move the current `bot.on('message', ...)` body into a middleware handler using
   the v2 filter signature `bot.on('message', async (ctx, next) => ...)`, then
   confirm the exact types against the installed package.
5. Replace event-specific error handling with `bot.catch(...)` and classify `TelegramApiError`, network, and timeout errors instead of matching v1 error strings.
6. Replace every positional send/download call with `bot.api` calls and explicit parameter objects.
7. Replace local-path uploads with `fromPath(path)` or `new InputFile(bytes, { filename })`; never pass a local path as a bare string because v2 treats strings as file IDs/URLs.
8. Replace `stopPolling()` with `bot.stop()` and keep SIGINT/SIGTERM cleanup idempotent.

## API Conversion Matrix

| Current v1 usage                            | v2 target                                                                             |
| ------------------------------------------- | ------------------------------------------------------------------------------------- |
| `new TelegramBot(token, { polling: true })` | `new Bot(token)` then `await run(bot)`                                                |
| `bot.on('message', handler)`                | v2 message middleware receiving `ctx`                                                 |
| `bot.on('polling_error', handler)`          | `bot.catch(handler)` plus transport error classification                              |
| `bot.sendMessage(chatId, text, opts)`       | `bot.api.sendMessage({ chat_id: chatId, text, ...opts })` or `ctx.reply`              |
| `bot.sendChatAction(chatId, action)`        | `bot.api.sendChatAction({ chat_id: chatId, action })`                                 |
| `bot.sendPhoto(chatId, path, opts)`         | `bot.api.sendPhoto({ chat_id: chatId, photo: await fromPath(path), ...opts })`        |
| `bot.sendDocument(chatId, path, opts)`      | `bot.api.sendDocument({ chat_id: chatId, document: await fromPath(path), ...opts })`  |
| `bot.getFileLink(fileId)`                   | `bot.api.getFile({ file_id: fileId })` plus an explicit download URL/request strategy |
| `bot.stopPolling()`                         | `bot.stop()`                                                                          |
| `bot.getMe()`                               | `bot.api.getMe()`                                                                     |

The `getFileLink` conversion needs a focused spike because v2 exposes the API call but does not necessarily provide the same convenience URL helper. Prefer a supported Node helper or a validated Telegram file URL built from the returned `file_path`; do not concatenate unvalidated user input into a filesystem path.

## Execution Phases

### Phase 1: Compatibility Spike

- Create a temporary v2 branch/worktree or isolated gateway entrypoint.
- Install `node-telegram-bot-api@^2.1.0`; remove `@types/node-telegram-bot-api`.
- Compile a minimal bot that starts polling, handles `/start`, sends text, uploads one photo, downloads one document, and shuts down.
- Confirm exact middleware signatures and `fromPath`/`InputFile` behavior from package types, not assumptions.

### Phase 2: Transport Adapter

- Add a small Telegram adapter module containing text, photo, document, chat-action, file-download, and lifecycle helpers.
- Keep daemon IPC, command registry, allowlist, formatting, and stream handling outside the adapter.
- Convert all call sites in `gateway.ts` to the adapter so v2-specific API details have one source of truth.

### Phase 3: Message and Command Middleware

- Port the existing message callback without changing command semantics.
- Preserve allowlist rejection, busy-session handling, document/image processing, and error replies.
- Ensure middleware calls `next()` only when the update is intentionally unhandled; do not double-process messages.

### Phase 4: Lifecycle, Errors, and Proxy

- Replace polling startup/shutdown and polling error listeners.
- Preserve graceful daemon disconnect and temporary-file cleanup.
- Port proxy behavior through the v2 `fetch` option using the existing proxy configuration.
- Add explicit handling for Telegram API rate limits and network timeouts.

### Phase 5: Dependency and Installer Update

- Update `baoclaw-telegram/package.json` and lockfile to v2.
- Remove `@types/node-telegram-bot-api`.
- Ensure `install.sh` installs the lockfile reproducibly and the launcher still targets `src/gateway.ts`.
- Run `npm audit` and record any remaining non-fixable advisories.

## Verification Checklist

- `npx tsc --noEmit` passes for `baoclaw-telegram`.
- Unit tests cover command routing, allowlist behavior, formatting, and error classification.
- Integration test with a fake Telegram transport verifies text, photo, document, typing, and file-download calls.
- Manual test with a real bot in a private chat verifies `/start`, normal text, long response splitting, tool output, image output, document input, and `/export`.
- Verify a rejected sender receives no response and does not reach the daemon.
- Verify SIGINT/SIGTERM stop polling once and close the daemon connection.
- Verify rate-limit and network errors do not terminate the process unexpectedly.
- Run `npm audit`, `npm test` where available, and the full installer smoke test.

## Risks and Rollback

- v2 has no compatibility shim; this is a transport rewrite, not a package-only bump.
- File upload/download semantics changed and are the highest-risk area.
- Middleware dispatch can accidentally duplicate or swallow updates if `next()` is mishandled.
- Keep the current `0.66.x` lockfile and gateway implementation until all manual tests pass.
- Rollback is a package-lock/package.json revert plus restoring the v1 transport adapter; daemon IPC changes should not be mixed into this migration.

## Recommended First Implementation Slice

Start with a separate `telegram-v2-adapter.ts` containing `sendText`, `sendPhoto`, `sendDocument`, `sendTyping`, `downloadTelegramFile`, `start`, and `stop`. Port one `/start` handler and a text reply first, then add media and daemon streaming after the basic polling loop is proven.
