import React from "react";
import { PrivyProvider } from "@privy-io/react-auth";
import Launchpad from "./Launchpad";
import type { Bootstrap } from "./App";

export default function PrivyLaunchpad({ bootstrap }: { bootstrap: Bootstrap }) {
  const appId = String(bootstrap.privy_app_id || "").trim();
  const clientId = String(bootstrap.privy_client_id || "").trim() || undefined;

  return (
    <PrivyProvider
      appId={appId}
      clientId={clientId}
      config={{
        loginMethods: ["wallet", "email"],
        embeddedWallets: {
          ethereum: { createOnLogin: "users-without-wallets" },
        },
        appearance: { theme: "dark" },
      }}
    >
      <Launchpad bootstrap={bootstrap} />
    </PrivyProvider>
  );
}
