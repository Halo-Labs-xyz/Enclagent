# Privy Agentic Wallets And TEE Migration

> ## Documentation Index
> Fetch the complete documentation index at: https://docs.privy.io/llms.txt
> Use this file to discover all available pages before exploring further.

## Agentic wallets

Privy enables developers to create wallets for AI agents and autonomous systems that can execute onchain transactions independently while maintaining strict policy controls and security guardrails.

Agentic wallets are designed for use cases where autonomous systems need to make decisions and execute transactions without human intervention, such as trading agents, portfolio managers, automated market makers, and autonomous service providers.

At a high-level, this recipe teaches developers how to:

- Set up wallets that AI agents can control.
- Implement policies to constrain agent behavior.
- Enable secure autonomous transaction execution.

### Step 1: Choose your control model

Privy supports two primary models for agentic wallets depending on custody and control requirements:

Model 1: Agent-controlled, developer-owned wallets

- Application backend controls the wallet via authorization keys.
- Suitable for fully autonomous agents where users delegate complete control.
- Agent can execute transactions within policy constraints without user approval.

Model 2: User-owned wallets with agent signers

- Users maintain ownership while granting limited permissions to agents.
- Agent operates as an additional signer with scoped policies.
- Users retain ultimate control and can revoke agent access at any time.

This recipe focuses on Model 1 for fully autonomous agents. For Model 2, use the signers guide:

- https://docs.privy.io/wallets/using-wallets/signers/overview

### Step 2: Create authorization keys

Set up authorization keys that the application backend will use to control agent wallets.

- Create authorization keys in the Privy Dashboard.
- Securely store the corresponding private keys.
- Backend uses these keys to sign requests to Privy APIs on behalf of agents.

For stronger security, register authorization keys in a key quorum for multi-party approval on critical actions like policy updates or wallet exports.

Reference docs:

- Create authorization keys: https://docs.privy.io/controls/authorization-keys/keys/create/key
- Key quorums quickstart: https://docs.privy.io/controls/key-quorum/create#nodejs

### Step 3: Define agent policies

Policies define boundaries for AI agent operation and are required to prevent unintended or harmful actions.

Common policy constraints:

- Transfer limits
- Allowlisted contracts
- Recipient restrictions
- Time-based controls
- Action-specific rules

After creating a policy, store its `id` for wallet assignment.

Reference docs:

- Policies overview: https://docs.privy.io/controls/policies/overview
- Create a policy: https://docs.privy.io/controls/policies/create-a-policy#nodejs

### Step 4: Create the agent wallet

Create a wallet owned by the authorization key and attach policy IDs:

- Set `owner_id` to the authorization key `id`.
- Set `policy_ids` to a singleton array with the policy `id`.

You can reuse the same policy ID across multiple wallets for fleet-wide controls.

Reference docs:

- Wallets quickstart: https://docs.privy.io/wallets/wallets/create/create-a-wallet#nodejs

### Step 5: Execute transactions

Use Privy APIs to send transactions, sign transactions, or sign messages.

Reference docs:

- Send EVM transaction: https://docs.privy.io/wallets/using-wallets/ethereum/send-a-transaction#nodejs
- Send Solana transaction: https://docs.privy.io/wallets/using-wallets/solana/send-a-transaction#nodejs
- Tier 2 chains: https://docs.privy.io/wallets/using-wallets/other-chains#nodejs

### Step 6: Monitor and observe agent behavior

Implement monitoring and logging to track actions and ensure expected operation. Privy provides webhooks for transaction events and balance changes.

Reference docs:

- Transaction webhooks: https://docs.privy.io/wallets/gas-and-asset-management/assets/transaction-event-webhooks
- Balance webhooks: https://docs.privy.io/wallets/gas-and-asset-management/assets/balance-event-webhooks

### Learn more

- OpenClaw integration: https://docs.privy.io/recipes/agent-integrations/openclaw-agentic-wallets
- x402 payments quickstart: https://docs.privy.io/recipes/agent-integrations/x402
- Hyperliquid quickstart: https://docs.privy.io/recipes/hyperliquid-guide
- Gas sponsorship: https://docs.privy.io/wallets/gas-and-asset-management/gas/overview

---

> ## Documentation Index
> Fetch the complete documentation index at: https://docs.privy.io/llms.txt
> Use this file to discover all available pages before exploring further.

## Migrating wallets from on-device to TEEs

Privy's security architecture uses secure execution environments so wallet private keys are reconstructed only temporarily for sensitive operations.

Privy supports two execution environments:

- TEE execution (trusted execution environments / secure enclaves)
- On-device execution (user device)

Each model prevents persistent storage of complete private keys.

### Feature support

TEE execution is required for:

- Tier 1 and Tier 2 chain support (Bitcoin, SUI, Cosmos, and others)
- Policy engine support for Ethereum and Solana constraints
- Server-side wallet access via signers

Unsupported with TEE (active development):

- Farcaster signers

### Migration guide

When TEE execution is enabled:

- All new wallets are created in TEEs.
- Existing on-device wallets migrate to TEEs on next user login.
- Migration is one-way.

Eligibility constraints:

- App cannot use Farcaster signers.
- If delegated actions are used, migration resets delegations and re-enablement via signers is required.
- App cannot be built on Flutter SDK or Unity SDK for migration workflows.

### Step 1: Identify app execution environment

In Privy Dashboard:

- Go to Wallets -> Advanced.
- Confirm environment shows either On-device or TEE enabled.
- On-device apps show option to request migration.

Dashboard link:

- https://dashboard.privy.io/apps?tab=advanced&page=wallets

### Step 2: Upgrade SDKs

TEE execution minimum versions:

Client SDKs:

- React: `@privy-io/react-auth@3.13.0`
- Expo: `@privy-io/expo@0.54.0`
- iOS (Swift): `2.0.0-beta.11`
- Android (Kotlin): `0.1.0-beta.1`
- Flutter: `0.1.0-beta.1`
- Unity: `0.6.0`

Server SDKs:

- Node: `@privy-io/node@0.1.0`
- Node (legacy): `@privy-io/server-auth@1.26.0`

Note:

- Flutter and Unity support TEE execution, but do not support migrating existing on-device wallets to TEEs. Contact support if needed: support@privy.io

### Step 3: Enable TEE execution in Dashboard

In Wallets -> Advanced:

- Select "Request access to migrate to TEE".
- Follow dashboard instructions.

Important migration notes:

- Client-side features are available immediately after migrated users log in.
- Server-side features are available only for migrated wallets.
- Some users may never return, leaving wallets on-device.
- User-managed recovery prompts are disabled after migration.
- Migration cannot be reversed.

### Optional: manually control migration timing

Default behavior is automatic migration on login. Manual migration is available by disabling automatic migration and calling migration hooks/methods.

React:

- Set `embeddedWallets.disableAutomaticMigration = true` in `PrivyProvider`.
- Use `useMigrateWallets` and call `migrate()`.

React Native:

- Set `embedded.disableAutomaticMigration = true` in `PrivyProvider`.
- Use `useMigrateWallets` from `@privy-io/expo` and call `migrate()`.

Android:

- Configure `disableAutomaticMigration = true` in `PrivyConfig`.
- Call `user?.migrateWalletsIfNeeded()`.

Swift:

- Set `disableAutomaticMigration = true` in `PrivyEmbeddedWalletConfig`.
- Call `try await user?.migrateWalletsIfNeeded()`.

### Breaking changes

TEE execution introduces a limited set of breaking changes.

#### Unsupported features

- See unsupported list: https://docs.privy.io/recipes/tee-wallet-migration-guide#unsupported-features

#### useSessionSigners interface migration

Use `useSigners` / `addSigners` instead of delegated-actions hooks for server-side wallet access and signer policy assignment.

Reference docs:

- Previous delegation flow: https://docs.privy.io/wallets/using-wallets/signers/delegate-wallet
- Previous revoke flow: https://docs.privy.io/wallets/using-wallets/signers/revoke-wallets
- Updated add signers: https://docs.privy.io/wallets/using-wallets/signers/add-signers
- Updated remove signers: https://docs.privy.io/wallets/using-wallets/signers/remove-signers

### New advanced interfaces

TEE execution adds deeper server-side wallet creation controls:

- Multiple chain wallet creation via unified `wallets` array
- Per-wallet `policy_ids`
- Additional signers with optional `override_policy_ids`
- Smart wallet creation via `create_smart_wallet` for Ethereum chain type

Applies to:

- `/users`
- `/users/import`
- `/users/[user_id]/wallets`

Reference docs:

- Users API reference: https://docs.privy.io/api-reference/users
- Create/import single user: https://docs.privy.io/user-management/migrating-users-to-privy/create-or-import-a-user
- Batch import users: https://docs.privy.io/user-management/migrating-users-to-privy/create-or-import-a-batch-of-users
- Pregenerate wallets: https://docs.privy.io/recipes/pregenerate-wallets
